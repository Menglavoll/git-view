//! 远程仓库相关 Tauri commands。

#![allow(clippy::needless_pass_by_value)]

use tauri::{AppHandle, State};

use crate::errors::Result;
use crate::models::git::CommitDetail;
use crate::models::repository::RemoteRepository;
use crate::services::account_service;
use crate::services::provider::{BranchHead, CommitPage};
use crate::services::repository_service::{self, RemoteRepoFilter};
use crate::services::tag_service::{
    self, BatchTagPrecheckResult, BatchTagRequest, BatchTagStartResult,
};
use crate::AppState;

/// 查询远程仓库列表（支持多条件筛选）。
#[tauri::command]
pub fn list_remote_repositories(
    state: State<'_, AppState>,
    filter: RemoteRepoFilter,
) -> Result<Vec<RemoteRepository>> {
    repository_service::list_remote_repositories(&state.db, &filter)
}

/// 搜索远程仓库（筛选 + 关键词）。
#[tauri::command]
pub fn search_remote_repositories(
    state: State<'_, AppState>,
    keyword: String,
    filter: RemoteRepoFilter,
) -> Result<Vec<RemoteRepository>> {
    let mut f = filter;
    f.search = Some(keyword);
    repository_service::list_remote_repositories(&state.db, &f)
}

/// 刷新远程仓库（触发同步）。
#[tauri::command]
pub async fn refresh_remote_repositories(
    state: State<'_, AppState>,
    account_id: Option<String>,
) -> Result<usize> {
    if let Some(id) = account_id {
        account_service::sync_account_repositories(&state.account_service_state, &state.db, &id)
            .await
    } else {
        let accounts = account_service::list_accounts(&state.db)?;
        let mut total = 0;
        for acc in accounts {
            if acc.enabled {
                // 单账号同步失败不阻塞其他账号；累计成功的仓库数
                if let Ok(n) = account_service::sync_account_repositories(
                    &state.account_service_state,
                    &state.db,
                    &acc.id,
                )
                .await
                {
                    total += n;
                }
            }
        }
        Ok(total)
    }
}

/// 获取单个远程仓库详情。
#[tauri::command]
pub fn get_remote_repository_detail(
    state: State<'_, AppState>,
    repo_id: String,
) -> Result<RemoteRepository> {
    repository_service::get_remote_repository(&state.db, &repo_id)
}

/// 切换远程仓库收藏状态。
#[tauri::command]
pub fn toggle_favorite_remote_repository(
    state: State<'_, AppState>,
    repo_id: String,
) -> Result<bool> {
    repository_service::toggle_favorite(&state.db, &repo_id)
}

/// 拉取远程仓库的提交历史（分页）。
///
/// 同步取出仓库与 provider（不跨 await 持有 DB 锁），再异步调平台 commits API。
/// page 从 1 起、默认每页 30 条，与三平台 API 的分页约定对齐。
#[tauri::command]
pub async fn list_remote_commits(
    state: State<'_, AppState>,
    repo_id: String,
    page: Option<u32>,
    per_page: Option<u32>,
) -> Result<CommitPage> {
    let repo = repository_service::get_remote_repository(&state.db, &repo_id)?;
    let provider = account_service::provider_for_account(&state.db, &repo.account_id)?;
    provider
        .list_commits(&repo, None, page.unwrap_or(1), per_page.unwrap_or(30))
        .await
}

/// 获取远程仓库单个提交的详情（元信息 + 改动文件 + 每文件 diff）。
#[tauri::command]
pub async fn get_remote_commit_detail(
    state: State<'_, AppState>,
    repo_id: String,
    sha: String,
) -> Result<CommitDetail> {
    let repo = repository_service::get_remote_repository(&state.db, &repo_id)?;
    let provider = account_service::provider_for_account(&state.db, &repo.account_id)?;
    provider.get_commit_detail(&repo, &sha).await
}

/// 列出远程仓库的全部分支名（供批量克隆时选择克隆分支）。
///
/// 同步取出仓库与 provider（不跨 await 持有 DB 锁），再异步调平台 branches API。
/// 平台若不支持分支列表，Provider 默认实现回退为仅含默认分支的单元素列表，
/// 保证前端下拉至少有默认分支可选、不阻断克隆。
#[tauri::command]
pub async fn list_remote_branches(
    state: State<'_, AppState>,
    repo_id: String,
) -> Result<Vec<String>> {
    let repo = repository_service::get_remote_repository(&state.db, &repo_id)?;
    let provider = account_service::provider_for_account(&state.db, &repo.account_id)?;
    provider.list_branches(&repo).await
}

/// 读取指定远程分支的最新提交，供批量 Tag 配置和确认摘要核对。
#[tauri::command]
pub async fn get_remote_branch_head(
    state: State<'_, AppState>,
    repo_id: String,
    branch: String,
) -> Result<BranchHead> {
    let repo = repository_service::get_remote_repository(&state.db, &repo_id)?;
    let provider = account_service::provider_for_account(&state.db, &repo.account_id)?;
    provider.get_branch_head(&repo, &branch).await
}

/// 批量 Tag 的只读预检查；不写远程仓库，也不记录写操作日志。
#[tauri::command]
pub async fn precheck_batch_tags(
    state: State<'_, AppState>,
    payload: BatchTagRequest,
) -> Result<Vec<BatchTagPrecheckResult>> {
    tag_service::precheck_batch_tags(&state.db, &payload).await
}

/// 确认后启动后台批量 Tag 创建。逐项目进度和最终结果通过 Tauri event 回传。
#[tauri::command]
pub fn start_batch_tags(
    app: AppHandle,
    state: State<'_, AppState>,
    payload: BatchTagRequest,
    prechecks: Vec<BatchTagPrecheckResult>,
) -> Result<BatchTagStartResult> {
    tag_service::start_batch_tags(
        app,
        state.db.clone(),
        state.batch_tag_manager.clone(),
        payload,
        prechecks,
    )
}

/// 请求停止尚未开始的项目；已提交给远端平台的请求会继续返回最终结果。
#[tauri::command]
pub fn cancel_batch_tags(state: State<'_, AppState>, batch_id: String) -> Result<()> {
    state.batch_tag_manager.cancel(&batch_id)
}
