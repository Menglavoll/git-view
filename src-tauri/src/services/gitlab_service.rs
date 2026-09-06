//! GitLab Provider 实现。
//!
//! 支持 gitlab.com 与自建 GitLab 实例：
//!   - 自签名证书白名单：仅对该实例生效，不污染全局 TLS 配置
//!   - 自定义代理：实例级独立配置
//!   - PRIVATE-TOKEN 头认证
//!
//! 同时提供 `derive_gitlab_api_url`：从 Web 地址自动推导 API 基址（T029）。

use std::time::Duration;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use reqwest::Client;
use serde::Deserialize;
use url::Url;
use uuid::Uuid;

use crate::errors::{GitViewError, Result};
use crate::models::account::GitPlatform;
use crate::models::git::{CommitDetail, CommitFile, CommitFileStatus, CommitStats, CommitSummary};
use crate::models::repository::{CreateRepoRequest, RemoteRepository, Visibility};
use crate::services::provider::{
    parse_iso_datetime, truncate_file_diff, BranchHead, CommitPage, CreatedRemoteTag,
    GitHostingProvider, RemoteTagType, RepositoryPage, UserProfile,
};
use crate::utils::redact::redact_token;

/// 请求超时（秒）。
const REQUEST_TIMEOUT_SECS: u64 = 30;

/// User-Agent。
const USER_AGENT: &str = "GitView/1.0";

/// GitLab Provider 构造时所需的实例级配置。
///
/// 与 `models::account::GitLabInstanceConfig` 解耦：本结构仅承载 Provider 所需
/// 运行时配置（不含数据库字段如 `id` / `created_at`）。`account_service`
/// 在实例化 Provider 前完成模型 → 配置的转换。
#[derive(Debug, Clone)]
pub struct GitLabClientConfig {
    /// API 基址（含 scheme/host/可选端口/可选子路径）
    pub api_base_url: String,
    /// 是否允许自签名 / 无效证书（仅对该实例生效）
    pub allow_invalid_certs: bool,
    /// 可选代理 URL
    pub proxy_url: Option<String>,
    /// 请求超时（秒），未提供时使用默认 30s
    pub request_timeout_seconds: Option<u64>,
}

/// GitLab Provider。
pub struct GitLabProvider {
    api_base_url: String,
    /// 私有 token —— 严禁暴露于 Debug 等输出
    token: String,
    client: Client,
}

impl GitLabProvider {
    /// 创建 GitLab Provider 实例。
    pub fn new(config: GitLabClientConfig, token: String) -> Result<Self> {
        let timeout = config
            .request_timeout_seconds
            .unwrap_or(REQUEST_TIMEOUT_SECS);

        let mut builder = Client::builder()
            .user_agent(USER_AGENT)
            .timeout(Duration::from_secs(timeout));

        if config.allow_invalid_certs {
            // 仅在本实例 Client 上启用，不影响全局
            builder = builder.danger_accept_invalid_certs(true);
        }

        if let Some(proxy_url) = config.proxy_url.as_deref() {
            let proxy = reqwest::Proxy::all(proxy_url).map_err(|e| {
                GitViewError::Network(format!("代理地址无效：{}", redact_token(&e.to_string())))
            })?;
            builder = builder.proxy(proxy);
        }

        let client = builder.build().map_err(|e| {
            GitViewError::Internal(format!(
                "HTTP 客户端初始化失败：{}",
                redact_token(&e.to_string())
            ))
        })?;

        Ok(Self {
            api_base_url: config.api_base_url,
            token,
            client,
        })
    }
}

/// GitLab `/user` 响应的最小字段子集。
#[derive(Debug, Deserialize)]
struct GitLabUserResp {
    /// 平台内部数值 ID（保留以便后续 ID 关联）
    #[allow(dead_code)]
    id: i64,
    username: String,
    name: Option<String>,
    avatar_url: Option<String>,
    /// 用户主页 URL（保留供 UI 跳转使用）
    #[allow(dead_code)]
    web_url: Option<String>,
}

/// GitLab `/projects` 响应中单个项目的字段子集。
#[derive(Debug, Deserialize)]
struct GitLabProjectResp {
    id: i64,
    path_with_namespace: String,
    name: String,
    namespace: GitLabNamespace,
    description: Option<String>,
    default_branch: Option<String>,
    /// 项目可见性；精简响应或低权限下可能缺省，用 Option 容错（缺省按私有处理）
    visibility: Option<String>,
    web_url: String,
    http_url_to_repo: String,
    ssh_url_to_repo: Option<String>,
    last_activity_at: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GitLabNamespace {
    path: String,
}

/// GitLab 提交对象字段子集（list 与单提交元信息共用）。
#[derive(Debug, Deserialize)]
struct GitLabCommitResp {
    id: String,
    short_id: String,
    title: Option<String>,
    message: Option<String>,
    author_name: Option<String>,
    author_email: Option<String>,
    authored_date: Option<String>,
    committer_name: Option<String>,
    committer_email: Option<String>,
    committed_date: Option<String>,
    #[serde(default)]
    parent_ids: Vec<String>,
    web_url: Option<String>,
    stats: Option<GitLabStats>,
}

/// GitLab 分支列表项字段子集（仅取分支名）。
#[derive(Debug, Deserialize)]
struct GitLabBranchResp {
    name: String,
}

/// GitLab 单分支响应包含 head commit，足以同时提供 SHA 与标题。
#[derive(Debug, Deserialize)]
struct GitLabBranchHeadResp {
    name: String,
    commit: GitLabBranchCommit,
}

#[derive(Debug, Deserialize)]
struct GitLabBranchCommit {
    id: String,
    title: Option<String>,
    message: Option<String>,
}

/// GitLab 单提交 diff 端点的单文件项（无 per-file 增删数）。
#[derive(Debug, Deserialize)]
struct GitLabDiffResp {
    old_path: String,
    new_path: String,
    diff: String,
    #[serde(default)]
    new_file: bool,
    #[serde(default)]
    deleted_file: bool,
    #[serde(default)]
    renamed_file: bool,
}

/// 提交增删行汇总。
#[derive(Debug, Deserialize)]
struct GitLabStats {
    additions: u32,
    deletions: u32,
    total: u32,
}

#[async_trait]
impl GitHostingProvider for GitLabProvider {
    async fn get_current_user(&self) -> Result<UserProfile> {
        let url = format!("{}/user", self.api_base_url.trim_end_matches('/'));

        let resp = self
            .client
            .get(&url)
            .header("PRIVATE-TOKEN", &self.token)
            .send()
            .await
            .map_err(|e| map_request_error("GitLab", &e))?;

        let status = resp.status();
        if !status.is_success() {
            return Err(map_status_error("GitLab", status.as_u16()));
        }

        let user: GitLabUserResp = resp.json().await.map_err(|e| {
            GitViewError::ResponseDecode(format!(
                "解析 GitLab 用户响应失败：{}",
                redact_token(&e.to_string())
            ))
        })?;

        Ok(UserProfile {
            username: user.username,
            display_name: user.name,
            avatar_url: user.avatar_url,
        })
    }

    async fn list_repositories(
        &self,
        page: u32,
        per_page: u32,
        account_id: &str,
    ) -> Result<RepositoryPage> {
        // 不使用 simple=true：精简响应不含 visibility 字段，会触发反序列化缺字段失败
        let url = format!(
            "{}/projects?membership=true&per_page={}&page={}&order_by=last_activity_at&sort=desc",
            self.api_base_url.trim_end_matches('/'),
            per_page,
            page,
        );

        let resp = self
            .client
            .get(&url)
            .header("PRIVATE-TOKEN", &self.token)
            .send()
            .await
            .map_err(|e| map_request_error("GitLab", &e))?;

        let status = resp.status();
        if !status.is_success() {
            return Err(map_status_error("GitLab", status.as_u16()));
        }

        let next_page = resp
            .headers()
            .get("x-next-page")
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.parse::<u32>().ok());

        let projects: Vec<GitLabProjectResp> = resp.json().await.map_err(|e| {
            GitViewError::ResponseDecode(format!(
                "解析 GitLab 项目列表失败：{}",
                redact_token(&e.to_string())
            ))
        })?;

        let has_next = next_page.is_some_and(|n| n > 0);
        let now = Utc::now();

        let items: Vec<RemoteRepository> = projects
            .into_iter()
            .map(|p| map_gitlab_project(p, account_id, now))
            .collect();

        Ok(RepositoryPage { items, has_next })
    }

    async fn list_commits(
        &self,
        repo: &RemoteRepository,
        branch: Option<&str>,
        page: u32,
        per_page: u32,
    ) -> Result<CommitPage> {
        // GitLab 用 project id（remote_id）拼 URL，分支用 ref_name
        let ref_name = branch.unwrap_or(&repo.default_branch);
        let url = format!(
            "{}/projects/{}/repository/commits?ref_name={}&per_page={}&page={}",
            self.api_base_url.trim_end_matches('/'),
            repo.remote_id,
            ref_name,
            per_page,
            page,
        );

        let resp = self
            .client
            .get(&url)
            .header("PRIVATE-TOKEN", &self.token)
            .send()
            .await
            .map_err(|e| map_request_error("GitLab", &e))?;

        let status = resp.status();
        if !status.is_success() {
            return Err(map_status_error("GitLab", status.as_u16()));
        }

        // x-next-page 非空且 > 0 表示还有下一页
        let next_page = resp
            .headers()
            .get("x-next-page")
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.parse::<u32>().ok());
        let has_next = next_page.is_some_and(|n| n > 0);

        let list: Vec<GitLabCommitResp> = resp.json().await.map_err(|e| {
            GitViewError::ResponseDecode(format!(
                "解析 GitLab 提交列表失败：{}",
                redact_token(&e.to_string())
            ))
        })?;

        let items = list
            .into_iter()
            .map(|c| {
                let authored_at = c
                    .authored_date
                    .as_deref()
                    .and_then(parse_iso_datetime)
                    .unwrap_or_else(Utc::now);
                CommitSummary {
                    short_sha: c.short_id,
                    summary: c.title.unwrap_or_default(),
                    author_name: c.author_name.unwrap_or_default(),
                    authored_at,
                    html_url: c.web_url,
                    sha: c.id,
                }
            })
            .collect();

        Ok(CommitPage { items, has_next })
    }

    async fn get_commit_detail(&self, repo: &RemoteRepository, sha: &str) -> Result<CommitDetail> {
        let base = self.api_base_url.trim_end_matches('/');

        // 1) 提交元信息（message / 作者 / 提交者 / parent_ids / stats）
        let meta_url = format!(
            "{base}/projects/{}/repository/commits/{sha}",
            repo.remote_id
        );
        let meta_resp = self
            .client
            .get(&meta_url)
            .header("PRIVATE-TOKEN", &self.token)
            .send()
            .await
            .map_err(|e| map_request_error("GitLab", &e))?;
        if !meta_resp.status().is_success() {
            return Err(map_status_error("GitLab", meta_resp.status().as_u16()));
        }
        let commit: GitLabCommitResp = meta_resp.json().await.map_err(|e| {
            GitViewError::ResponseDecode(format!(
                "解析 GitLab 提交详情失败：{}",
                redact_token(&e.to_string())
            ))
        })?;

        // 2) 提交 diff（GitLab 单独端点；每文件无增删数，只有 diff 文本与状态标志）
        let diff_url = format!(
            "{base}/projects/{}/repository/commits/{sha}/diff",
            repo.remote_id
        );
        let diff_resp = self
            .client
            .get(&diff_url)
            .header("PRIVATE-TOKEN", &self.token)
            .send()
            .await
            .map_err(|e| map_request_error("GitLab", &e))?;
        if !diff_resp.status().is_success() {
            return Err(map_status_error("GitLab", diff_resp.status().as_u16()));
        }
        let diffs: Vec<GitLabDiffResp> = diff_resp.json().await.map_err(|e| {
            GitViewError::ResponseDecode(format!(
                "解析 GitLab 提交 diff 失败：{}",
                redact_token(&e.to_string())
            ))
        })?;

        Ok(map_gitlab_detail(commit, diffs))
    }

    async fn create_repository(
        &self,
        req: &CreateRepoRequest,
        account_id: &str,
    ) -> Result<RemoteRepository> {
        let url = format!("{}/projects", self.api_base_url.trim_end_matches('/'));
        // initialize_with_readme=false 建空仓；GitLab 原生支持三态可见性，直接透传。
        let visibility = match req.visibility {
            Visibility::Public => "public",
            Visibility::Internal => "internal",
            Visibility::Private => "private",
        };
        let body = serde_json::json!({
            "name": req.name,
            "path": req.name,
            "description": req.description,
            "visibility": visibility,
            "initialize_with_readme": false,
        });

        let resp = self
            .client
            .post(&url)
            .header("PRIVATE-TOKEN", &self.token)
            .json(&body)
            .send()
            .await
            .map_err(|e| map_request_error("GitLab", &e))?;

        let status = resp.status();
        if !status.is_success() {
            // GitLab 重名以 400 返回，body 含 "has already been taken"
            if status.as_u16() == 400 {
                let detail = redact_token(&resp.text().await.unwrap_or_default());
                if detail.contains("has already been taken") {
                    return Err(GitViewError::RepoNameTaken);
                }
                return Err(GitViewError::Network(format!(
                    "GitLab 创建项目失败：{detail}"
                )));
            }
            return Err(map_status_error("GitLab", status.as_u16()));
        }

        let project: GitLabProjectResp = resp.json().await.map_err(|e| {
            GitViewError::ResponseDecode(format!(
                "解析 GitLab 创建项目响应失败：{}",
                redact_token(&e.to_string())
            ))
        })?;

        Ok(map_gitlab_project(project, account_id, Utc::now()))
    }

    async fn list_branches(&self, repo: &RemoteRepository) -> Result<Vec<String>> {
        // GitLab 用 project id（remote_id）拼 URL；分支接口按页返回，最多 100/页。
        // 用 x-next-page 头判断是否还有下一页，逐页聚合直到取完（克隆选分支需完整列表）。
        let mut branches = Vec::new();
        let mut page = 1u32;
        loop {
            let url = format!(
                "{}/projects/{}/repository/branches?per_page=100&page={}",
                self.api_base_url.trim_end_matches('/'),
                repo.remote_id,
                page,
            );

            let resp = self
                .client
                .get(&url)
                .header("PRIVATE-TOKEN", &self.token)
                .send()
                .await
                .map_err(|e| map_request_error("GitLab", &e))?;

            let status = resp.status();
            if !status.is_success() {
                return Err(map_status_error("GitLab", status.as_u16()));
            }

            // x-next-page 非空且 > 0 表示还有下一页（消费 body 前先取头）
            let next_page = resp
                .headers()
                .get("x-next-page")
                .and_then(|v| v.to_str().ok())
                .and_then(|s| s.parse::<u32>().ok());
            let has_next = next_page.is_some_and(|n| n > 0);

            let list: Vec<GitLabBranchResp> = resp.json().await.map_err(|e| {
                GitViewError::ResponseDecode(format!(
                    "解析 GitLab 分支列表失败：{}",
                    redact_token(&e.to_string())
                ))
            })?;

            branches.extend(list.into_iter().map(|b| b.name));

            if !has_next {
                break;
            }
            page += 1;
        }

        Ok(branches)
    }

    async fn get_branch_head(&self, repo: &RemoteRepository, branch: &str) -> Result<BranchHead> {
        let encoded: String = url::form_urlencoded::byte_serialize(branch.as_bytes()).collect();
        let url = format!(
            "{}/projects/{}/repository/branches/{}",
            self.api_base_url.trim_end_matches('/'),
            repo.remote_id,
            encoded
        );
        let resp = self
            .client
            .get(&url)
            .header("PRIVATE-TOKEN", &self.token)
            .send()
            .await
            .map_err(|e| map_request_error("GitLab", &e))?;
        let status = resp.status();
        if !status.is_success() {
            return if status.as_u16() == 404 {
                Err(GitViewError::BranchNotFound(branch.to_string()))
            } else {
                Err(map_status_error("GitLab", status.as_u16()))
            };
        }
        let item: GitLabBranchHeadResp = resp.json().await.map_err(|e| {
            GitViewError::ResponseDecode(format!(
                "解析 GitLab 分支响应失败：{}",
                redact_token(&e.to_string())
            ))
        })?;
        let subject = item
            .commit
            .title
            .or(item.commit.message)
            .unwrap_or_default()
            .lines()
            .next()
            .unwrap_or_default()
            .to_string();
        Ok(BranchHead {
            branch: item.name,
            short_sha: item.commit.id.chars().take(8).collect(),
            sha: item.commit.id,
            subject,
        })
    }

    async fn tag_exists(&self, repo: &RemoteRepository, tag_name: &str) -> Result<bool> {
        let encoded: String = url::form_urlencoded::byte_serialize(tag_name.as_bytes()).collect();
        let url = format!(
            "{}/projects/{}/repository/tags/{}",
            self.api_base_url.trim_end_matches('/'),
            repo.remote_id,
            encoded
        );
        let resp = self
            .client
            .get(&url)
            .header("PRIVATE-TOKEN", &self.token)
            .send()
            .await
            .map_err(|e| map_request_error("GitLab", &e))?;
        match resp.status().as_u16() {
            200 => Ok(true),
            404 => Ok(false),
            status => Err(map_status_error("GitLab", status)),
        }
    }

    async fn create_tag(
        &self,
        repo: &RemoteRepository,
        branch: &str,
        tag_name: &str,
        tag_type: RemoteTagType,
        message: Option<&str>,
    ) -> Result<CreatedRemoteTag> {
        let head = self.get_branch_head(repo, branch).await?;
        if self.tag_exists(repo, tag_name).await? {
            return Err(GitViewError::TagAlreadyExists(tag_name.to_string()));
        }
        let url = format!(
            "{}/projects/{}/repository/tags",
            self.api_base_url.trim_end_matches('/'),
            repo.remote_id
        );
        let mut query = vec![("tag_name", tag_name), ("ref", branch)];
        if matches!(tag_type, RemoteTagType::Annotated) {
            query.push(("message", message.unwrap_or_default()));
        }
        let resp = self
            .client
            .post(&url)
            .header("PRIVATE-TOKEN", &self.token)
            .query(&query)
            .send()
            .await
            .map_err(|e| map_request_error("GitLab", &e))?;
        match resp.status().as_u16() {
            200 | 201 => Ok(CreatedRemoteTag {
                name: tag_name.to_string(),
                target_sha: head.sha,
            }),
            400 | 405 | 409 => Err(GitViewError::TagAlreadyExists(tag_name.to_string())),
            429 => Err(GitViewError::RateLimited("GitLab".to_string())),
            status => Err(map_status_error("GitLab", status)),
        }
    }
}

// =====================================================================
// API URL 推导（T029）
// =====================================================================

/// 根据 GitLab Web 地址推导 API 地址。
///
/// 规则：
///   - 保留 scheme、host、端口
///   - 在已有路径末尾追加 `/api/v4`（子路径部署时如 `https://x/gitlab` →
///     `https://x/gitlab/api/v4`）
///   - 末尾 `/` 规范化（避免出现双斜杠）
///
/// # Examples
///
/// ```
/// use gitview_lib::services::gitlab_service::derive_gitlab_api_url;
/// assert_eq!(
///     derive_gitlab_api_url("https://gitlab.com").unwrap(),
///     "https://gitlab.com/api/v4"
/// );
/// assert_eq!(
///     derive_gitlab_api_url("https://code.company.com/gitlab/").unwrap(),
///     "https://code.company.com/gitlab/api/v4"
/// );
/// ```
pub fn derive_gitlab_api_url(web_url: &str) -> Result<String> {
    let url = Url::parse(web_url)
        .map_err(|e| GitViewError::ApiUrlInvalid(format!("Web 地址无法解析：{e}")))?;

    // 校验 scheme（仅允许 http/https）
    if !matches!(url.scheme(), "http" | "https") {
        return Err(GitViewError::ApiUrlInvalid(format!(
            "仅支持 http/https，实际：{}",
            url.scheme()
        )));
    }

    if url.host_str().is_none() {
        return Err(GitViewError::ApiUrlInvalid("缺少 host".to_string()));
    }

    // 组装：scheme://host:port/<existing-path>/api/v4
    let mut base = format!("{}://{}", url.scheme(), url.host_str().unwrap_or(""));
    if let Some(port) = url.port() {
        base.push(':');
        base.push_str(&port.to_string());
    }

    // 已有路径（去掉首尾斜杠后再统一拼接）
    let path = url.path().trim_matches('/');
    if path.is_empty() {
        Ok(format!("{base}/api/v4"))
    } else {
        Ok(format!("{base}/{path}/api/v4"))
    }
}

// =====================================================================
// 错误映射（与 GitHub Provider 共用语义，但保留独立函数便于平台前缀注入）
// =====================================================================

/// 把 GitLab 项目响应映射为统一的 `RemoteRepository`（list 与 create 共用）。
fn map_gitlab_project(
    p: GitLabProjectResp,
    account_id: &str,
    now: DateTime<Utc>,
) -> RemoteRepository {
    let pushed_at = p
        .last_activity_at
        .as_deref()
        .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
        .map(|dt| dt.with_timezone(&Utc));
    let visibility = match p.visibility.as_deref() {
        Some("public") => Visibility::Public,
        Some("internal") => Visibility::Internal,
        // 缺省 / private / 未知值一律按私有处理（安全默认，避免误暴露）
        _ => Visibility::Private,
    };
    RemoteRepository {
        id: Uuid::new_v4().to_string(),
        account_id: account_id.to_string(),
        platform: GitPlatform::Gitlab,
        remote_id: p.id.to_string(),
        full_name: p.path_with_namespace,
        name: p.name,
        owner: p.namespace.path,
        description: p.description,
        visibility,
        default_branch: p.default_branch.unwrap_or_else(|| "main".to_string()),
        html_url: p.web_url,
        ssh_url: p.ssh_url_to_repo,
        clone_url: p.http_url_to_repo,
        is_favorite: false,
        last_pushed_at: pushed_at,
        synced_at: now,
    }
}

/// 把 GitLab 提交元信息 + diff 端点结果合并为统一的 `CommitDetail`。
fn map_gitlab_detail(c: GitLabCommitResp, diffs: Vec<GitLabDiffResp>) -> CommitDetail {
    let authored_at = c
        .authored_date
        .as_deref()
        .and_then(parse_iso_datetime)
        .unwrap_or_else(Utc::now);
    let committed_at = c.committed_date.as_deref().and_then(parse_iso_datetime);
    let files = diffs.into_iter().map(map_gitlab_file).collect();
    CommitDetail {
        short_sha: c.short_id,
        sha: c.id,
        // message 含完整正文，缺失时回退 title
        message: c.message.or(c.title).unwrap_or_default(),
        author_name: c.author_name.unwrap_or_default(),
        author_email: c.author_email.unwrap_or_default(),
        authored_at,
        committer_name: c.committer_name,
        committer_email: c.committer_email,
        committed_at,
        parent_shas: c.parent_ids,
        html_url: c.web_url,
        stats: c.stats.map(|s| CommitStats {
            additions: s.additions,
            deletions: s.deletions,
            total: s.total,
        }),
        files,
    }
}

/// 把 GitLab diff 项映射为统一的 `CommitFile`（每文件增删数 GitLab 不提供，置 None）。
fn map_gitlab_file(d: GitLabDiffResp) -> CommitFile {
    let status = if d.new_file {
        CommitFileStatus::Added
    } else if d.deleted_file {
        CommitFileStatus::Deleted
    } else if d.renamed_file {
        CommitFileStatus::Renamed
    } else {
        CommitFileStatus::Modified
    };
    // 仅重命名时保留旧路径
    let old_path = if d.renamed_file {
        Some(d.old_path)
    } else {
        None
    };
    let (diff, truncated) = truncate_file_diff(&d.diff);
    CommitFile {
        path: d.new_path,
        old_path,
        status,
        additions: None,
        deletions: None,
        diff: Some(diff),
        truncated,
    }
}

fn map_request_error(platform: &str, e: &reqwest::Error) -> GitViewError {
    if e.is_timeout() {
        return GitViewError::Network(format!("{platform} 请求超时"));
    }
    if e.is_connect() {
        return GitViewError::Network(format!("{platform} 连接失败，请检查网络或代理"));
    }
    // TLS 错误专项识别（GitLab 自签名场景特别相关）
    let msg = e.to_string();
    if msg.contains("certificate") || msg.contains("TLS") || msg.contains("tls") {
        return GitViewError::TlsCert(redact_token(&msg));
    }
    GitViewError::Network(redact_token(&msg))
}

fn map_status_error(platform: &str, status: u16) -> GitViewError {
    match status {
        401 => GitViewError::TokenInvalid,
        // GitLab 会用 403 表示 Token 被撤销，与 GitHub 的 401 语义不完全一致 —— 这里仍保留 Forbidden 语义，
        // 上层 UI 可结合错误细节给出"请重新生成 Token"提示
        403 => GitViewError::Forbidden,
        404 => GitViewError::NotFound(format!("{platform} 资源不存在")),
        _ => GitViewError::Network(format!("{platform} 请求失败，HTTP {status}")),
    }
}

// =====================================================================
// 单元测试
// =====================================================================
#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    #[test]
    fn derive_url_standard_gitlab_com() {
        assert_eq!(
            derive_gitlab_api_url("https://gitlab.com").unwrap(),
            "https://gitlab.com/api/v4"
        );
        assert_eq!(
            derive_gitlab_api_url("https://gitlab.com/").unwrap(),
            "https://gitlab.com/api/v4"
        );
    }

    #[test]
    fn derive_url_custom_port() {
        assert_eq!(
            derive_gitlab_api_url("https://gitlab.company.com:8443").unwrap(),
            "https://gitlab.company.com:8443/api/v4"
        );
    }

    #[test]
    fn derive_url_http_intranet() {
        assert_eq!(
            derive_gitlab_api_url("http://10.0.0.5").unwrap(),
            "http://10.0.0.5/api/v4"
        );
    }

    #[test]
    fn derive_url_subpath_deployment() {
        assert_eq!(
            derive_gitlab_api_url("https://code.company.com/gitlab").unwrap(),
            "https://code.company.com/gitlab/api/v4"
        );
        assert_eq!(
            derive_gitlab_api_url("https://code.company.com/gitlab/").unwrap(),
            "https://code.company.com/gitlab/api/v4"
        );
    }

    #[test]
    fn derive_url_rejects_invalid_scheme() {
        let err = derive_gitlab_api_url("ftp://gitlab.com").unwrap_err();
        assert!(matches!(err, GitViewError::ApiUrlInvalid(_)));
    }

    #[test]
    fn derive_url_rejects_unparseable() {
        let err = derive_gitlab_api_url("not a url").unwrap_err();
        assert!(matches!(err, GitViewError::ApiUrlInvalid(_)));
    }

    #[test]
    fn status_error_mapping() {
        assert!(matches!(
            map_status_error("GitLab", 401),
            GitViewError::TokenInvalid
        ));
        assert!(matches!(
            map_status_error("GitLab", 403),
            GitViewError::Forbidden
        ));
    }

    #[test]
    fn provider_constructor_with_self_signed() {
        let cfg = GitLabClientConfig {
            api_base_url: "https://gitlab.example.com/api/v4".to_string(),
            allow_invalid_certs: true,
            proxy_url: None,
            request_timeout_seconds: Some(10),
        };
        let provider = GitLabProvider::new(cfg, "glpat-test_token_for_unit".to_string());
        assert!(provider.is_ok());
    }
}
