//! `GitView` 后端库入口。
//!
//! 本模块是 Tauri 应用的核心组装点，负责：
//!   - 初始化结构化日志系统（tracing + 按天滚动文件）
//!   - 注入应用全局状态（`AppState`：数据库连接池等）
//!   - 启动期执行数据库迁移与凭据存储可用性诊断
//!   - 注册 Tauri 官方插件与自定义 IPC command
//!
//! 模块组织（按 Phase 逐步追加）：
//!   - `errors`   — 统一错误类型（T013 ✓）
//!   - `db`       — 数据库连接池与迁移（T015 ✓ / T016 ✓）
//!   - `models`   — 领域模型（T017 ✓）
//!   - `services` — 业务服务层（T023 起）
//!   - `utils`    — 工具模块（T018 ✓）
//!   - `commands` — Tauri IPC 命令（Phase 3+）

pub mod commands;
pub mod db;
pub mod errors;
pub mod models;
pub mod services;
pub mod utils;

use tracing_appender::rolling;
use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

use crate::db::pool::DbPool;
use crate::services::account_service::AccountServiceState;
use crate::services::clone_task_service::CloneManager;

// =====================================================================
// 应用全局状态（T019）
// =====================================================================

/// Tauri 应用全局状态。
///
/// 通过 `Builder::manage(AppState)` 注入后，所有 `#[tauri::command]`
/// 可通过 `tauri::State<AppState>` 参数访问。
pub struct AppState {
    /// SQLite 连接池（克隆代价低：`Arc<Mutex<Connection>>`）
    pub db: DbPool,
    /// 账号服务运行时状态（同步互斥锁集合）
    pub account_service_state: AccountServiceState,
    /// Clone 任务调度器（持有 git CLI + semaphore + cancel token map）
    pub clone_manager: CloneManager,
}

// =====================================================================
// 日志初始化（T014）
// =====================================================================

/// 初始化 tracing 日志系统。
///
/// 日志同时输出到：
///   1. 按天滚动的文件（位于 `<data_local_dir>/gitview/logs/`）
///   2. 开发模式下的 stdout（便于 `tauri dev` 调试）
///
/// 日志级别通过环境变量 `RUST_LOG` 控制，默认 `info`。
///
/// # Returns
///
/// 返回 `WorkerGuard`，调用方必须持有该 guard 直到应用退出，
/// 否则异步写入的日志可能丢失。
fn init_tracing() -> tracing_appender::non_blocking::WorkerGuard {
    // 日志目录：macOS ~/Library/Application Support/gitview/logs
    //           Windows %LOCALAPPDATA%/gitview/logs
    //           Linux ~/.local/share/gitview/logs
    let log_dir = crate::utils::path::log_dir();

    // 按天滚动的文件 appender
    let file_appender = rolling::daily(&log_dir, "gitview.log");
    let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);

    // 环境变量过滤器，默认 info 级别
    let env_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

    // 文件输出层：JSON 格式便于后续日志分析
    let file_layer = fmt::layer()
        .with_writer(non_blocking)
        .with_ansi(false)
        .with_target(true)
        .with_thread_ids(true);

    // stdout 输出层：仅在 debug 构建中启用，带颜色便于开发调试
    let stdout_layer = fmt::layer()
        .with_writer(std::io::stdout)
        .with_ansi(true)
        .with_target(false);

    tracing_subscriber::registry()
        .with(env_filter)
        .with(file_layer)
        .with(stdout_layer)
        .init();

    guard
}

// =====================================================================
// 启动期初始化（T019）
// =====================================================================

/// 构建 `AppState`：打开数据库、执行迁移、诊断凭据存储。
///
/// # Errors
///
/// 任意子步骤失败均返回错误；启动期错误由 `run` 直接 `panic`，
/// 因为此类问题用户无法在 UI 上恢复（参考 T019 设计）。
fn init_app_state() -> errors::Result<AppState> {
    let db_path = DbPool::default_path();
    tracing::info!("打开数据库：{}", db_path.display());
    let db = DbPool::new(&db_path)?;

    let applied = db::migrations::run_pending_migrations(&db)?;
    tracing::info!("迁移已应用 {applied} 个");

    if let Err(e) = services::credential_service::check_availability() {
        tracing::warn!("凭据存储不可用：{e}（部分账号功能将受限）");
    } else {
        tracing::info!("凭据存储可用");
    }

    // 启动期回扫：把上次会话遗留的 running/pending 任务标记为 failed
    match services::clone_task_service::reconcile_orphan_tasks(&db) {
        Ok(0) => {}
        Ok(n) => tracing::info!("回扫 {n} 个孤儿 clone 任务为 failed"),
        Err(e) => tracing::warn!("回扫孤儿任务失败：{e}"),
    }

    // CloneManager 当前是占位实现；无论 tokio runtime 是否已初始化都返回 placeholder。
    // 未来若需要区分（如在已有 runtime 时复用 Handle），可拆分两支。
    let clone_manager = CloneManager::placeholder();

    Ok(AppState {
        db,
        account_service_state: AccountServiceState::new(),
        clone_manager,
    })
}

// =====================================================================
// Tauri 应用入口
// =====================================================================

/// 启动 Tauri 桌面应用。
///
/// 该函数由 `main.rs` 调用；亦可在集成测试中以 mock 状态注入后调用，
/// 用于端到端验证 command 注册与生命周期。
///
/// # Panics
///
/// 当数据库初始化、迁移执行或 Tauri 运行时初始化失败时 panic 退出，
/// 因为此类启动期错误用户无法恢复。
#[allow(clippy::large_stack_frames, clippy::expect_used)]
pub fn run() {
    // 初始化日志系统，guard 必须存活到应用退出
    let _guard = init_tracing();

    tracing::info!("GitView 启动中，日志系统已初始化");

    tauri::Builder::default()
        // 注册官方插件
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_os::init())
        // 应用内自动更新：从 GitHub Releases 拉取 latest.json、验签后原地安装
        .plugin(tauri_plugin_updater::Builder::new().build())
        // 更新安装完成后重启应用（process 插件提供 relaunch 能力）
        .plugin(tauri_plugin_process::init())
        // 应用启动阶段：执行迁移、注入 AppState
        .setup(|app| {
            // 引入 Manager trait 以使用 manage() 方法
            use tauri::Manager;
            let state = init_app_state().expect("应用启动初始化失败");
            tracing::info!("Tauri setup 完成，数据库已就绪");
            app.manage(state);
            Ok(())
        })
        // 注册 IPC 命令（Phase 3 起，按 User Story 增量追加）
        .invoke_handler(tauri::generate_handler![
            // US1 账号管理
            commands::accounts::add_account,
            commands::accounts::test_account_connection,
            commands::accounts::list_accounts,
            commands::accounts::update_account,
            commands::accounts::delete_account,
            commands::accounts::set_default_account,
            commands::accounts::sync_account_repositories,
            // US2 远程仓库
            commands::remote_repositories::list_remote_repositories,
            commands::remote_repositories::search_remote_repositories,
            commands::remote_repositories::refresh_remote_repositories,
            commands::remote_repositories::get_remote_repository_detail,
            commands::remote_repositories::toggle_favorite_remote_repository,
            commands::remote_repositories::list_remote_commits,
            commands::remote_repositories::get_remote_commit_detail,
            commands::remote_repositories::list_remote_branches,
            commands::remote_repositories::get_remote_branch_head,
            commands::remote_repositories::precheck_batch_tags,
            commands::remote_repositories::create_batch_tags,
            // US3 Clone 任务
            commands::clone_tasks::create_clone_tasks,
            commands::clone_tasks::start_clone_tasks,
            commands::clone_tasks::list_clone_tasks,
            commands::clone_tasks::cancel_clone_task,
            commands::clone_tasks::retry_clone_task,
            commands::clone_tasks::clear_clone_tasks_by_status,
            // US4 本地仓库
            commands::local_repositories::add_local_repository,
            commands::local_repositories::scan_local_repositories,
            commands::local_repositories::list_local_repositories,
            commands::local_repositories::remove_local_repository,
            commands::local_repositories::refresh_local_repository_status,
            commands::local_repositories::refresh_all_local_repository_status,
            commands::local_repositories::batch_fetch_repositories,
            commands::local_repositories::open_repository_folder,
            commands::local_repositories::open_repository_in_terminal,
            commands::local_repositories::publish_local_repository,
            commands::local_repositories::set_local_repository_protocol,
            // US5 单仓库 Git 工作流（15 个）
            commands::git::git_status,
            commands::git::git_diff,
            commands::git::git_list_branches,
            commands::git::git_log,
            commands::git::git_stage_file,
            commands::git::git_unstage_file,
            commands::git::git_stage_all,
            commands::git::git_unstage_all,
            commands::git::git_commit,
            commands::git::git_fetch,
            commands::git::git_pull,
            commands::git::git_push,
            commands::git::git_checkout_branch,
            commands::git::git_create_branch,
            commands::git::git_discard_changes,
            commands::git::git_commit_detail,
            // US6 操作日志
            commands::logs::list_operation_logs,
            commands::logs::get_operation_log_detail,
            commands::logs::clear_old_operation_logs,
            // US7 设置与默认目录
            commands::settings::get_settings,
            commands::settings::update_settings,
            commands::settings::detect_git,
            commands::settings::set_git_path,
            commands::settings::get_log_stats,
            commands::settings::clear_old_logs,
            // US7 凭据级命令（账号与安全 / FR-055）
            commands::accounts::check_credential_exists,
            commands::accounts::save_credential,
            commands::accounts::delete_credential,
            // US7 应用数据目录迁移
            commands::data_dir::get_data_dir,
            commands::data_dir::migrate_data_dir,
            commands::data_dir::restore_default_data_dir,
            commands::data_dir::get_old_data_dir,
            commands::data_dir::delete_old_data_dir,
            commands::data_dir::restart_app,
        ])
        .run(tauri::generate_context!())
        .expect("启动 GitView 应用失败：Tauri Builder.run() 异常");
}
