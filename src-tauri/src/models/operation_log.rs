//! 操作日志领域模型。
//!
//! 记录用户在 GitView 中执行的关键操作，便于故障复盘与审计。
//! V1 范围限定为 spec.md 中列出的操作类型，不含 V2 的 merge/rebase。

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// 操作类型枚举。
///
/// V1 范围（与 tasks.md T017 验收标准一致）：
/// `add_account`、`delete_account`、`test_connection`、`sync_repos`、
/// `clone`、`fetch`、`pull`、`push`、`commit`、`checkout`、`create_branch`、
/// `scan_repos`、`discard_changes`。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationType {
    /// 添加账号
    AddAccount,
    /// 删除账号
    DeleteAccount,
    /// 测试账号连接
    TestConnection,
    /// 同步远程仓库列表
    SyncRepos,
    /// 克隆仓库
    Clone,
    /// fetch 远端
    Fetch,
    /// pull 远端
    Pull,
    /// push 到远端
    Push,
    /// 提交变更
    Commit,
    /// 切换分支
    Checkout,
    /// 新建分支
    CreateBranch,
    /// 批量创建 Tag 的汇总记录。
    BatchCreateTag,
    /// 单个远程仓库创建 Tag 的明细记录。
    CreateTag,
    /// 扫描本地仓库目录
    ScanRepos,
    /// 丢弃工作区变更
    DiscardChanges,
}

/// 操作结果状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationStatus {
    /// 成功
    Success,
    /// 失败
    Failed,
    /// 用户取消
    Cancelled,
}

/// 操作日志条目。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OperationLog {
    /// 日志唯一标识（UUID v4）
    pub id: String,
    /// 操作类型
    pub operation_type: OperationType,
    /// 操作目标的简短描述（如仓库全名、账号用户名，已脱敏）
    pub target: String,
    /// 操作结果状态
    pub status: OperationStatus,
    /// 执行的命令（已脱敏，如 `git push origin main`），用于诊断复盘
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    /// 命令输出摘要（已脱敏，成功时为 stdout 摘要），用于诊断复盘
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output: Option<String>,
    /// 错误信息（失败时填充，已脱敏）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_message: Option<String>,
    /// 错误信息的中文翻译。
    ///
    /// 不入库：查询时由 `log_service::translate_error` 基于 `error_message`
    /// 动态计算填充，以便翻译表升级后历史日志也能享受新翻译。
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub translated_error_message: Option<String>,
    /// 操作耗时（毫秒）
    pub duration_ms: u64,
    /// 操作发生时间
    pub occurred_at: DateTime<Utc>,
}

/// 操作日志查询筛选条件。
///
/// 所有维度可选：空 `Vec` 或 `None` 表示该维度不限制。
/// 分页 `page` 从 0 开始，`page_size` 默认 50。
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct LogFilter {
    /// 按操作类型筛选（空表示全部类型）
    pub operation_types: Vec<OperationType>,
    /// 按结果状态筛选（空表示全部状态）
    pub statuses: Vec<OperationStatus>,
    /// 关键字，匹配 `target` 字段（`LIKE` 模糊匹配）
    pub keyword: Option<String>,
    /// 页码（从 0 开始）
    pub page: u32,
    /// 每页条数
    pub page_size: u32,
}

impl Default for LogFilter {
    /// 默认筛选：不限类型/状态/关键字，取第一页、每页 50 条。
    fn default() -> Self {
        Self {
            operation_types: Vec::new(),
            statuses: Vec::new(),
            keyword: None,
            page: 0,
            page_size: 50,
        }
    }
}
