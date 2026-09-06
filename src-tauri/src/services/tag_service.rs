//! 远程仓库批量创建 Tag 服务。
//!
//! 本模块把前端配置转换为受控的 Provider 调用：预检查不写远端，正式创建前再次
//! 校验分支与同名 Tag。批次并非分布式事务，单个项目失败必须隔离并返回明细。

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use serde::{Deserialize, Serialize};
use tauri::Emitter;
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::db::pool::DbPool;
use crate::errors::{GitViewError, Result};
use crate::models::operation_log::{OperationStatus, OperationType};
use crate::models::repository::RemoteRepository;
use crate::services::account_service;
use crate::services::log_service;
use crate::services::provider::{BranchHead, RemoteTagType};
use crate::services::repository_service;

pub const DEFAULT_CONCURRENCY: u32 = 3;
pub const MAX_CONCURRENCY: u32 = 20;
pub const MAX_ANNOTATION_CHARS: usize = 2000;
pub const BATCH_TAG_PROGRESS_EVENT: &str = "batch-tag-progress";
pub const BATCH_TAG_FINISHED_EVENT: &str = "batch-tag-finished";

/// 运行中批次的可取消调度器。取消令牌只阻止尚未提交到远端平台的项目；
/// 已开始的 HTTP 请求仍会等待其真实结果，符合远端 API 不可撤销的限制。
#[derive(Clone, Default)]
pub struct BatchTagManager {
    handles: Arc<Mutex<HashMap<String, CancellationToken>>>,
}

impl BatchTagManager {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    fn insert(&self, batch_id: String, cancel: CancellationToken) -> Result<()> {
        {
            let mut handles = self.handles.lock().map_err(|error| {
                GitViewError::Internal(format!("批量 Tag 调度器锁损坏：{error}"))
            })?;
            handles.insert(batch_id, cancel);
        }
        Ok(())
    }

    fn remove(&self, batch_id: &str) {
        if let Ok(mut handles) = self.handles.lock() {
            handles.remove(batch_id);
        }
    }

    pub fn cancel(&self, batch_id: &str) -> Result<()> {
        let cancel = self
            .handles
            .lock()
            .map_err(|error| GitViewError::Internal(format!("批量 Tag 调度器锁损坏：{error}")))?
            .get(batch_id)
            .cloned()
            .ok_or_else(|| GitViewError::NotFound(format!("批量 Tag 任务 {batch_id} 未在运行")))?;
        cancel.cancel();
        Ok(())
    }
}

/// 前端为一个远程仓库提交的 Tag 配置。
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchTagItem {
    pub repo_id: String,
    pub branch: String,
    pub tag_name: String,
    pub tag_type: RemoteTagType,
    #[serde(default)]
    pub message: Option<String>,
}

/// 一次批量操作请求。配置只驻留在内存，应用重启后不恢复。
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchTagRequest {
    pub items: Vec<BatchTagItem>,
    #[serde(default = "default_concurrency")]
    pub concurrency: u32,
}

const fn default_concurrency() -> u32 {
    DEFAULT_CONCURRENCY
}

/// 单项目预检查结果。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchTagPrecheckResult {
    pub repo_id: String,
    pub repo_name: String,
    pub account_name: String,
    pub branch: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub head: Option<BranchHead>,
    pub tag_name: String,
    pub tag_type: RemoteTagType,
    pub can_create: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_code: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

/// 单项目最终状态。`prechecked_sha` 与 `actual_sha` 不同表示分支在执行期间变化。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchTagItemResult {
    pub repo_id: String,
    pub repo_name: String,
    pub branch: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prechecked_sha: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub actual_sha: Option<String>,
    pub tag_name: String,
    pub tag_type: RemoteTagType,
    pub status: OperationStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_code: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_message: Option<String>,
    pub duration_ms: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchTagResult {
    pub total: usize,
    pub success: usize,
    pub failed: usize,
    pub cancelled: usize,
    pub items: Vec<BatchTagItemResult>,
}

/// 后台批次启动后的临时 ID。批次及其结果只在当前应用进程内有效。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchTagStartResult {
    pub batch_id: String,
}

/// 前端用于显示排队、执行、完成三类实时状态；最终状态仍写入 `BatchTagItemResult`。
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BatchTagExecutionStatus {
    Running,
    Success,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchTagProgressPayload {
    pub batch_id: String,
    pub total: usize,
    pub completed: usize,
    pub success: usize,
    pub failed: usize,
    pub cancelled: usize,
    pub repo_id: String,
    pub status: BatchTagExecutionStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<BatchTagItemResult>,
}

/// 校验请求中不依赖网络的部分，保证每一条项目配置可独立得到可读失败原因。
pub fn validate_request(request: &BatchTagRequest) -> Result<()> {
    if request.items.is_empty() {
        return Err(GitViewError::Internal("至少选择一个远程项目".to_string()));
    }
    if !(1..=MAX_CONCURRENCY).contains(&request.concurrency) {
        return Err(GitViewError::Internal(format!(
            "并发数必须在 1～{MAX_CONCURRENCY} 之间"
        )));
    }
    let mut seen = HashSet::new();
    for item in &request.items {
        if !seen.insert(&item.repo_id) {
            return Err(GitViewError::Internal("批量请求中存在重复项目".to_string()));
        }
        validate_item(item)?;
    }
    Ok(())
}

/// Git ref 的保守校验：后端是最终防线，前端即时校验只用于改善交互。
pub fn validate_tag_name(tag_name: &str) -> Result<()> {
    let name = tag_name.trim();
    if name.is_empty() || matches!(name, "." | "..") {
        return Err(GitViewError::TagNameInvalid(
            "名称不能为空、. 或 ..".to_string(),
        ));
    }
    if name.starts_with('/')
        || name.ends_with('/')
        || name.contains("..")
        || name.contains("@{")
        || std::path::Path::new(name)
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("lock"))
    {
        return Err(GitViewError::TagNameInvalid(
            "不符合 Git ref 路径规则".to_string(),
        ));
    }
    if name.chars().any(|c| {
        c.is_control() || c.is_whitespace() || matches!(c, '~' | '^' | ':' | '?' | '*' | '[' | '\\')
    }) {
        return Err(GitViewError::TagNameInvalid(
            "包含 Git ref 不允许的字符".to_string(),
        ));
    }
    Ok(())
}

fn validate_item(item: &BatchTagItem) -> Result<()> {
    if item.repo_id.trim().is_empty() || item.branch.trim().is_empty() {
        return Err(GitViewError::Internal("项目和分支不能为空".to_string()));
    }
    validate_tag_name(&item.tag_name)?;
    match item.tag_type {
        RemoteTagType::Lightweight => Ok(()),
        RemoteTagType::Annotated => {
            let count = item.message.as_deref().unwrap_or_default().chars().count();
            if !(1..=MAX_ANNOTATION_CHARS).contains(&count) {
                return Err(GitViewError::Internal(format!(
                    "附注 Tag 说明长度必须为 1～{MAX_ANNOTATION_CHARS} 个字符"
                )));
            }
            Ok(())
        }
    }
}

/// 对所有项目执行只读预检查。单项失败被封装为结果，不阻断其他项目。
pub async fn precheck_batch_tags(
    pool: &DbPool,
    request: &BatchTagRequest,
) -> Result<Vec<BatchTagPrecheckResult>> {
    validate_request(request)?;
    let mut results = Vec::with_capacity(request.items.len());
    for item in &request.items {
        results.push(precheck_one(pool, item).await);
    }
    Ok(results)
}

async fn precheck_one(pool: &DbPool, item: &BatchTagItem) -> BatchTagPrecheckResult {
    let base =
        |repo_name: String, account_name: String, err: GitViewError| BatchTagPrecheckResult {
            repo_id: item.repo_id.clone(),
            repo_name,
            account_name,
            branch: item.branch.clone(),
            head: None,
            tag_name: item.tag_name.clone(),
            tag_type: item.tag_type,
            can_create: false,
            error_code: Some(error_code(&err).to_string()),
            message: Some(err.to_string()),
        };
    if let Err(err) = validate_item(item) {
        return base(item.repo_id.clone(), "—".to_string(), err);
    }
    let repo = match repository_service::get_remote_repository(pool, &item.repo_id) {
        Ok(repo) => repo,
        Err(err) => return base(item.repo_id.clone(), "—".to_string(), err),
    };
    let account_name = account_service::list_accounts(pool)
        .ok()
        .and_then(|items| {
            items
                .into_iter()
                .find(|a| a.id == repo.account_id)
                .map(|a| a.username)
        })
        .unwrap_or_else(|| "—".to_string());
    let provider = match account_service::provider_for_account(pool, &repo.account_id) {
        Ok(provider) => provider,
        Err(err) => return base(repo.full_name, account_name, err),
    };
    if !provider.supports_tag_type(item.tag_type) {
        return base(
            repo.full_name,
            account_name,
            GitViewError::AnnotatedTagUnsupported("当前平台 API".to_string()),
        );
    }
    let head = match provider.get_branch_head(&repo, &item.branch).await {
        Ok(head) => head,
        Err(err) => return base(repo.full_name, account_name, err),
    };
    match provider.tag_exists(&repo, &item.tag_name).await {
        Ok(true) => base(
            repo.full_name,
            account_name,
            GitViewError::TagAlreadyExists(item.tag_name.clone()),
        ),
        Ok(false) => BatchTagPrecheckResult {
            repo_id: item.repo_id.clone(),
            repo_name: repo.full_name,
            account_name,
            branch: item.branch.clone(),
            head: Some(head),
            tag_name: item.tag_name.clone(),
            tag_type: item.tag_type,
            can_create: true,
            error_code: None,
            message: None,
        },
        Err(err) => base(repo.full_name, account_name, err),
    }
}

/// 启动后台批量创建。正式执行不信任旧预检查结果，仍由 Provider 再次检查冲突与分支 head。
///
/// 调用立即返回批次 ID；进度和最终结果通过 Tauri event 回传，避免长时间 IPC 阻塞。
pub fn start_batch_tags<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    pool: DbPool,
    manager: BatchTagManager,
    request: BatchTagRequest,
    prechecks: Vec<BatchTagPrecheckResult>,
) -> Result<BatchTagStartResult> {
    validate_request(&request)?;
    if !prechecks_match_request(&request, &prechecks) {
        return Err(GitViewError::Internal(
            "预检查结果与当前批量 Tag 配置不一致，请重新预检查".to_string(),
        ));
    }
    if prechecks.iter().any(|result| !result.can_create) {
        return Err(GitViewError::Internal(
            "存在预检查失败项目，不能创建 Tag".to_string(),
        ));
    }
    let batch_id = Uuid::new_v4().to_string();
    let cancel = CancellationToken::new();
    manager.insert(batch_id.clone(), cancel.clone())?;
    let started = BatchTagStartResult {
        batch_id: batch_id.clone(),
    };

    tauri::async_runtime::spawn(async move {
        run_batch_tags(app, pool, manager, batch_id, request, prechecks, cancel).await;
    });
    Ok(started)
}

/// 预检查是确认窗口的安全门：每个项目必须有且仅有一条、并与当前配置一致。
/// 真正创建时 Provider 仍会再次检查，因为预检查与执行之间远端状态可能变化。
fn prechecks_match_request(
    request: &BatchTagRequest,
    prechecks: &[BatchTagPrecheckResult],
) -> bool {
    if request.items.len() != prechecks.len() {
        return false;
    }
    let checks_by_repo: HashMap<_, _> = prechecks
        .iter()
        .map(|check| (check.repo_id.as_str(), check))
        .collect();
    checks_by_repo.len() == prechecks.len()
        && request.items.iter().all(|item| {
            checks_by_repo
                .get(item.repo_id.as_str())
                .is_some_and(|check| {
                    check.branch == item.branch
                        && check.tag_name == item.tag_name
                        && check.tag_type == item.tag_type
                })
        })
}

#[allow(clippy::too_many_lines)]
async fn run_batch_tags<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    pool: DbPool,
    manager: BatchTagManager,
    batch_id: String,
    request: BatchTagRequest,
    prechecks: Vec<BatchTagPrecheckResult>,
    cancel: CancellationToken,
) {
    let started = Instant::now();
    let prechecked_shas: HashMap<_, _> = prechecks
        .iter()
        .filter_map(|r| r.head.as_ref().map(|h| (r.repo_id.clone(), h.sha.clone())))
        .collect();
    let precheck_names: HashMap<_, _> = prechecks
        .iter()
        .map(|result| (result.repo_id.clone(), result.repo_name.clone()))
        .collect();
    let total = request.items.len();
    let mut queued: VecDeque<_> = request.items.into();
    let mut tasks = JoinSet::new();
    let mut items = Vec::with_capacity(total);
    let mut success = 0;
    let mut failed = 0;
    let mut cancelled = 0;

    while !queued.is_empty() || !tasks.is_empty() {
        while !cancel.is_cancelled() && tasks.len() < request.concurrency as usize {
            let Some(item) = queued.pop_front() else {
                break;
            };
            emit_progress(
                &app,
                &batch_id,
                total,
                items.len(),
                success,
                failed,
                cancelled,
                &item.repo_id,
                BatchTagExecutionStatus::Running,
                None,
            );
            let db = pool.clone();
            let prechecked_sha = prechecked_shas.get(&item.repo_id).cloned();
            tasks.spawn(async move { create_one(&db, item, prechecked_sha).await });
        }

        if cancel.is_cancelled() {
            while let Some(item) = queued.pop_front() {
                let repo_id = item.repo_id.clone();
                let result = cancelled_result(
                    item,
                    precheck_names.get(&repo_id).cloned(),
                    prechecked_shas.get(&repo_id).cloned(),
                );
                let result_repo_id = result.repo_id.clone();
                cancelled += 1;
                items.push(result.clone());
                emit_progress(
                    &app,
                    &batch_id,
                    total,
                    items.len(),
                    success,
                    failed,
                    cancelled,
                    &result_repo_id,
                    BatchTagExecutionStatus::Cancelled,
                    Some(result),
                );
            }
        }

        let Some(joined) = tasks.join_next().await else {
            continue;
        };
        let Ok(result) = joined else {
            tracing::error!("批量 Tag 工作线程异常：{joined:?}");
            continue;
        };
        let result_repo_id = result.repo_id.clone();
        let status = match result.status {
            OperationStatus::Success => {
                success += 1;
                BatchTagExecutionStatus::Success
            }
            OperationStatus::Failed => {
                failed += 1;
                BatchTagExecutionStatus::Failed
            }
            OperationStatus::Cancelled => {
                cancelled += 1;
                BatchTagExecutionStatus::Cancelled
            }
        };
        items.push(result.clone());
        emit_progress(
            &app,
            &batch_id,
            total,
            items.len(),
            success,
            failed,
            cancelled,
            &result_repo_id,
            status,
            Some(result),
        );
    }

    let result = BatchTagResult {
        total,
        success,
        failed,
        cancelled,
        items,
    };
    let summary_status = if result.failed > 0 {
        OperationStatus::Failed
    } else if result.cancelled > 0 {
        OperationStatus::Cancelled
    } else {
        OperationStatus::Success
    };
    let summary = format!(
        "total={} success={} failed={} cancelled={}",
        result.total, result.success, result.failed, result.cancelled
    );
    let duration_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
    let _ = log_service::record_operation(
        &pool,
        OperationType::BatchCreateTag,
        "批量创建 Tag",
        summary_status,
        Some("batch_create_tag"),
        Some(&summary),
        None,
        duration_ms,
    );
    let _ = app.emit(
        BATCH_TAG_FINISHED_EVENT,
        BatchTagFinishedPayload {
            batch_id: batch_id.clone(),
            result,
        },
    );
    manager.remove(&batch_id);
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct BatchTagFinishedPayload {
    batch_id: String,
    result: BatchTagResult,
}

#[allow(clippy::too_many_arguments)]
fn emit_progress<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    batch_id: &str,
    total: usize,
    completed: usize,
    success: usize,
    failed: usize,
    cancelled: usize,
    repo_id: &str,
    status: BatchTagExecutionStatus,
    result: Option<BatchTagItemResult>,
) {
    let _ = app.emit(
        BATCH_TAG_PROGRESS_EVENT,
        BatchTagProgressPayload {
            batch_id: batch_id.to_string(),
            total,
            completed,
            success,
            failed,
            cancelled,
            repo_id: repo_id.to_string(),
            status,
            result,
        },
    );
}

fn cancelled_result(
    item: BatchTagItem,
    repo_name: Option<String>,
    prechecked_sha: Option<String>,
) -> BatchTagItemResult {
    BatchTagItemResult {
        repo_id: item.repo_id,
        repo_name: repo_name.unwrap_or_else(|| "—".to_string()),
        branch: item.branch,
        prechecked_sha,
        actual_sha: None,
        tag_name: item.tag_name,
        tag_type: item.tag_type,
        status: OperationStatus::Cancelled,
        error_code: None,
        error_message: Some("已取消，未向远程平台发起创建请求".to_string()),
        duration_ms: 0,
    }
}

async fn create_one(
    pool: &DbPool,
    item: BatchTagItem,
    prechecked_sha: Option<String>,
) -> BatchTagItemResult {
    let started = Instant::now();
    let repo = match repository_service::get_remote_repository(pool, &item.repo_id) {
        Ok(repo) => repo,
        Err(err) => {
            let repo_name = item.repo_id.clone();
            return failure_result(item, repo_name, prechecked_sha, &err, started);
        }
    };
    let repo_name = repo.full_name.clone();
    let result = create_on_provider(pool, &repo, &item).await;
    let duration_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
    match result {
        Ok(created) => {
            let command = format!(
                "create_tag branch={} tag={} type={:?}",
                item.branch, item.tag_name, item.tag_type
            );
            let _ = log_service::record_operation(
                pool,
                OperationType::CreateTag,
                &repo_name,
                OperationStatus::Success,
                Some(&command),
                None,
                None,
                duration_ms,
            );
            BatchTagItemResult {
                repo_id: item.repo_id,
                repo_name,
                branch: item.branch,
                prechecked_sha,
                actual_sha: Some(created.target_sha),
                tag_name: item.tag_name,
                tag_type: item.tag_type,
                status: OperationStatus::Success,
                error_code: None,
                error_message: None,
                duration_ms,
            }
        }
        Err(err) => {
            let _ = log_service::record_operation(
                pool,
                OperationType::CreateTag,
                &repo_name,
                OperationStatus::Failed,
                None,
                None,
                Some(&err.to_string()),
                duration_ms,
            );
            BatchTagItemResult {
                repo_id: item.repo_id,
                repo_name,
                branch: item.branch,
                prechecked_sha,
                actual_sha: None,
                tag_name: item.tag_name,
                tag_type: item.tag_type,
                status: OperationStatus::Failed,
                error_code: Some(error_code(&err).to_string()),
                error_message: Some(err.to_string()),
                duration_ms,
            }
        }
    }
}

async fn create_on_provider(
    pool: &DbPool,
    repo: &RemoteRepository,
    item: &BatchTagItem,
) -> Result<crate::services::provider::CreatedRemoteTag> {
    let provider = account_service::provider_for_account(pool, &repo.account_id)?;
    provider
        .create_tag(
            repo,
            &item.branch,
            &item.tag_name,
            item.tag_type,
            item.message.as_deref(),
        )
        .await
}

fn failure_result(
    item: BatchTagItem,
    repo_name: String,
    prechecked_sha: Option<String>,
    err: &GitViewError,
    started: Instant,
) -> BatchTagItemResult {
    let duration_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
    BatchTagItemResult {
        repo_id: item.repo_id,
        repo_name,
        branch: item.branch,
        prechecked_sha,
        actual_sha: None,
        tag_name: item.tag_name,
        tag_type: item.tag_type,
        status: OperationStatus::Failed,
        error_code: Some(error_code(err).to_string()),
        error_message: Some(err.to_string()),
        duration_ms,
    }
}

const fn error_code(err: &GitViewError) -> &'static str {
    match err {
        GitViewError::TagNameInvalid(_) => "tag_name_invalid",
        GitViewError::TagAlreadyExists(_) => "tag_already_exists",
        GitViewError::BranchNotFound(_) => "branch_not_found",
        GitViewError::AnnotatedTagUnsupported(_) => "annotated_tag_unsupported",
        GitViewError::RateLimited(_) => "rate_limited",
        GitViewError::Forbidden => "permission_denied",
        GitViewError::NotFound(_) => "repo_not_found",
        _ => "unknown",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tag_name_validator_rejects_unsafe_refs() {
        assert!(validate_tag_name("v1.2.0").is_ok());
        assert!(validate_tag_name("release/2026-09").is_ok());
        assert!(validate_tag_name("bad name").is_err());
        assert!(validate_tag_name("release..bad").is_err());
        assert!(validate_tag_name("release.lock").is_err());
    }

    #[test]
    fn prechecks_must_match_current_request() {
        let request = BatchTagRequest {
            items: vec![BatchTagItem {
                repo_id: "repo-1".to_string(),
                branch: "main".to_string(),
                tag_name: "v1.0.0".to_string(),
                tag_type: RemoteTagType::Lightweight,
                message: None,
            }],
            concurrency: 3,
        };
        let check = BatchTagPrecheckResult {
            repo_id: "repo-1".to_string(),
            repo_name: "owner/repo".to_string(),
            account_name: "user".to_string(),
            branch: "main".to_string(),
            head: None,
            tag_name: "v1.0.0".to_string(),
            tag_type: RemoteTagType::Lightweight,
            can_create: true,
            error_code: None,
            message: None,
        };
        assert!(prechecks_match_request(
            &request,
            std::slice::from_ref(&check)
        ));

        let mut stale = check;
        stale.tag_name = "v0.9.0".to_string();
        assert!(!prechecks_match_request(&request, &[stale]));
        assert!(!prechecks_match_request(&request, &[]));
    }
}
