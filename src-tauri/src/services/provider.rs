//! Git 托管平台 Provider 抽象。
//!
//! 通过一个共同的 trait 统一三大平台（GitHub / GitLab / Gitee）的接口，
//! 上层 `account_service` 无需关心具体平台差异。
//!
//! 各平台实现位于：
//!   - [`crate::services::github_service`]
//!   - [`crate::services::gitlab_service`]
//!   - [`crate::services::gitee_service`]
//!
//! 安全约束（宪法 Principle III）：
//!   - Provider 实现的构造函数接收 token 后，必须将其存储为不可序列化字段
//!   - Provider 任何 `Debug`/`Display` 输出均不得包含 token
//!   - 错误消息通过 `crate::utils::redact::redact_token` 脱敏后再向上传递

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::errors::{GitViewError, Result};
use crate::models::git::{CommitDetail, CommitSummary};
use crate::models::repository::{CreateRepoRequest, RemoteRepository};

/// 远程 Tag 的类型。轻量 Tag 直接指向提交；附注 Tag 带说明与平台身份元数据。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RemoteTagType {
    Lightweight,
    Annotated,
}

/// 指定分支当前最新提交，供用户确认创建 Tag 的基准。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BranchHead {
    pub branch: String,
    pub sha: String,
    pub short_sha: String,
    pub subject: String,
}

/// 平台创建 Tag 后的统一回执。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreatedRemoteTag {
    pub name: String,
    pub target_sha: String,
}

/// 平台用户档案（连接测试与账号同步用）。
///
/// 跨平台统一字段集合 —— 各 Provider 负责把 API 响应映射到本结构。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserProfile {
    /// 平台用户名（如 `octocat`）
    pub username: String,
    /// 用户显示名（GitHub `name` / GitLab `name` / Gitee `name`）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    /// 头像 URL
    #[serde(skip_serializing_if = "Option::is_none")]
    pub avatar_url: Option<String>,
}

/// 仓库分页结果（list_repositories 使用，US2 起填充实际数据）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RepositoryPage {
    /// 当前页仓库列表
    pub items: Vec<RemoteRepository>,
    /// 是否还有下一页（用于前端"加载更多"判断）
    pub has_next: bool,
}

/// 提交分页结果（list_commits 使用）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommitPage {
    /// 当前页提交列表
    pub items: Vec<CommitSummary>,
    /// 是否还有下一页（用于前端"加载更多"判断）
    pub has_next: bool,
}

/// Git 托管平台 Provider 抽象。
///
/// 实现者负责：
///   - 持有该账号的 token 与 HTTP Client 配置
///   - 把 HTTP 错误映射为 `GitViewError` 的对应变体
///   - 把平台 API 响应映射到统一模型
///
/// `Send + Sync` 约束允许 Provider 在 `Arc` 共享下跨线程使用，
/// 满足异步任务并发调用。
#[async_trait]
pub trait GitHostingProvider: Send + Sync {
    /// 获取当前 token 对应的用户档案。
    ///
    /// 用于"测试连接"按钮与账号添加流程的身份确认。
    async fn get_current_user(&self) -> Result<UserProfile>;

    /// 拉取当前账号的仓库列表（分页）。
    ///
    /// `account_id` 用于把返回的 `RemoteRepository` 关联到本地数据库账号行；
    /// Provider 实现负责把平台 API 响应映射为统一的 `RemoteRepository` 结构。
    ///
    /// 默认实现返回空页，便于尚未实现该方法的 Provider 编译通过。
    async fn list_repositories(
        &self,
        _page: u32,
        _per_page: u32,
        _account_id: &str,
    ) -> Result<RepositoryPage> {
        Ok(RepositoryPage {
            items: Vec::new(),
            has_next: false,
        })
    }

    /// 拉取指定远程仓库的提交历史（分页）。
    ///
    /// `branch` 缺省时由实现回退到仓库默认分支。默认实现表示该平台尚未支持。
    async fn list_commits(
        &self,
        _repo: &RemoteRepository,
        _branch: Option<&str>,
        _page: u32,
        _per_page: u32,
    ) -> Result<CommitPage> {
        Err(GitViewError::Internal("该平台暂不支持提交历史".to_string()))
    }

    /// 列出远程仓库的所有分支名（用于「克隆时选择分支」）。
    ///
    /// 返回纯分支名列表（不含 `refs/heads/` 前缀）。默认实现返回仅含
    /// 仓库默认分支的单元素列表：这样即便某平台尚未实现真正的分支列表 API，
    /// 前端仍至少能选到默认分支，克隆功能不被阻断（见 research.md 决策）。
    async fn list_branches(&self, repo: &RemoteRepository) -> Result<Vec<String>> {
        Ok(vec![repo.default_branch.clone()])
    }

    /// 查询指定远程分支的最新提交。默认实现明确表示平台未支持，避免创建 Tag 时猜测 SHA。
    async fn get_branch_head(&self, _repo: &RemoteRepository, _branch: &str) -> Result<BranchHead> {
        Err(GitViewError::Internal(
            "该平台暂不支持读取分支最新提交".to_string(),
        ))
    }

    /// 检查同名 Tag 是否已经存在。`true` 表示冲突，创建流程绝不覆盖。
    async fn tag_exists(&self, _repo: &RemoteRepository, _tag_name: &str) -> Result<bool> {
        Err(GitViewError::Internal("该平台暂不支持检查 Tag".to_string()))
    }

    /// 是否支持目标 Tag 类型。默认支持；能力不足的 Provider 必须在预检查阶段拒绝，
    /// 而不能在用户确认后静默降级为另一类型。
    fn supports_tag_type(&self, _tag_type: RemoteTagType) -> bool {
        true
    }

    /// 基于指定分支创建 Tag。实现方负责在写入前重新解析分支 head 与冲突状态。
    async fn create_tag(
        &self,
        _repo: &RemoteRepository,
        _branch: &str,
        _tag_name: &str,
        _tag_type: RemoteTagType,
        _message: Option<&str>,
    ) -> Result<CreatedRemoteTag> {
        Err(GitViewError::Internal("该平台暂不支持创建 Tag".to_string()))
    }

    /// 获取单个提交的详情（元信息 + 改动文件 + 每文件 diff）。
    ///
    /// 默认实现表示该平台尚未支持。
    async fn get_commit_detail(
        &self,
        _repo: &RemoteRepository,
        _sha: &str,
    ) -> Result<CommitDetail> {
        Err(GitViewError::Internal("该平台暂不支持提交详情".to_string()))
    }

    /// 在平台创建一个**空**远程仓库，返回映射后的 `RemoteRepository`。
    ///
    /// `account_id` 用于把返回结果关联到本地账号行（与 `list_repositories` 一致）。
    /// 实现必须关闭平台自动初始化（不带 README / 初始 commit）——否则远程已有提交，
    /// 「发布到远程」后续的 `git push` 会因 non-fast-forward 失败。
    ///
    /// 默认实现表示该平台尚未支持创建仓库。
    async fn create_repository(
        &self,
        _req: &CreateRepoRequest,
        _account_id: &str,
    ) -> Result<RemoteRepository> {
        Err(GitViewError::Internal("该平台暂不支持创建仓库".to_string()))
    }
}

/// 单文件 diff 最大保留字符数，超出则截断（防止巨型提交撑爆 IPC / 前端渲染）。
pub const MAX_COMMIT_FILE_DIFF_CHARS: usize = 100_000;

/// 按字符数截断单文件 diff；返回 `(截断后文本, 是否发生截断)`。
#[must_use]
pub fn truncate_file_diff(diff: &str) -> (String, bool) {
    if diff.chars().count() > MAX_COMMIT_FILE_DIFF_CHARS {
        // 按字符（非字节）截断，避免切到多字节 UTF-8 中间导致 panic
        let truncated: String = diff.chars().take(MAX_COMMIT_FILE_DIFF_CHARS).collect();
        (truncated, true)
    } else {
        (diff.to_string(), false)
    }
}

/// 解析 ISO 8601 时间为 `DateTime<Utc>`；解析失败返回 `None`。
#[must_use]
pub fn parse_iso_datetime(s: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(s)
        .ok()
        .map(|dt| dt.with_timezone(&Utc))
}
