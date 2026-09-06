// =====================================================================
// 批量创建远程 Tag 的前端契约。
// 与 src-tauri/src/services/tag_service.rs 保持 camelCase 对齐；这些状态仅在
// 当前弹窗会话中保存，应用重启后不恢复。
// =====================================================================

export type RemoteTagType = 'lightweight' | 'annotated';

/** 指定远程分支当前最新提交，用于用户核对 Tag 创建基准。 */
export interface BranchHead {
  branch: string;
  sha: string;
  shortSha: string;
  subject: string;
}

/** 一个仓库的 Tag 配置。 */
export interface BatchTagItem {
  repoId: string;
  branch: string;
  tagName: string;
  tagType: RemoteTagType;
  message?: string;
}

export interface BatchTagRequest {
  items: BatchTagItem[];
  concurrency: number;
}

export interface BatchTagPrecheckResult {
  repoId: string;
  repoName: string;
  accountName: string;
  branch: string;
  head?: BranchHead;
  tagName: string;
  tagType: RemoteTagType;
  canCreate: boolean;
  errorCode?: string;
  message?: string;
}

export type BatchTagItemStatus = 'success' | 'failed' | 'cancelled';

export interface BatchTagItemResult {
  repoId: string;
  repoName: string;
  branch: string;
  precheckedSha?: string;
  actualSha?: string;
  tagName: string;
  tagType: RemoteTagType;
  status: BatchTagItemStatus;
  errorCode?: string;
  errorMessage?: string;
  durationMs: number;
}

export interface BatchTagResult {
  total: number;
  success: number;
  failed: number;
  cancelled: number;
  items: BatchTagItemResult[];
}
