// =====================================================================
// 操作日志类型
// 与 src-tauri/src/models/operation_log.rs 对齐（serde camelCase）。
// =====================================================================

/**
 * 操作类型（V1 范围，不含 V2 的 merge/rebase）。
 */
export type OperationType =
  | 'add_account'
  | 'delete_account'
  | 'test_connection'
  | 'sync_repos'
  | 'clone'
  | 'fetch'
  | 'pull'
  | 'push'
  | 'commit'
  | 'checkout'
  | 'create_branch'
  | 'batch_create_tag'
  | 'create_tag'
  | 'scan_repos'
  | 'discard_changes';

/** 操作结果状态。 */
export type OperationStatus = 'success' | 'failed' | 'cancelled';

/** 操作日志条目（字段与后端 OperationLog 一一对应）。 */
export interface OperationLog {
  /** 日志唯一标识（UUID v4） */
  id: string;
  /** 操作类型 */
  operationType: OperationType;
  /** 操作目标的简短描述（仓库名 / 账号名，已脱敏） */
  target: string;
  /** 操作结果状态 */
  status: OperationStatus;
  /** 执行的命令（已脱敏，可空），用于诊断复盘 */
  command?: string;
  /** 命令输出摘要（已脱敏，可空），用于诊断复盘 */
  output?: string;
  /** 错误信息（失败时填充，已脱敏，可空） */
  errorMessage?: string;
  /**
   * 错误信息的中文翻译（可空）。
   * 后端查询时由 log_service::translate_error 动态计算，不入库。
   */
  translatedErrorMessage?: string;
  /** 耗时毫秒 */
  durationMs: number;
  /** 操作发生时间（ISO 8601） */
  occurredAt: string;
}

/**
 * 操作日志查询筛选条件（与后端 LogFilter 对齐）。
 *
 * 所有维度可选：空数组 / 空关键字表示该维度不限制。
 * 分页 `page` 从 0 开始，`pageSize` 默认 50。
 */
export interface LogFilter {
  /** 按操作类型筛选（空数组表示全部类型） */
  operationTypes: OperationType[];
  /** 按结果状态筛选（空数组表示全部状态） */
  statuses: OperationStatus[];
  /** 关键字，模糊匹配 target 字段 */
  keyword?: string;
  /** 页码（从 0 开始） */
  page: number;
  /** 每页条数 */
  pageSize: number;
}
