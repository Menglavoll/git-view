# 前后端接口契约：批量创建 Tag

## 前端 API

建议扩展 `src/api/remoteRepository.api.ts`：

```ts
listBranches(repoId: string): Promise<RemoteBranch[]>;
getBranchHead(repoId: string, branch: string): Promise<BranchHead>;
precheckBatchTags(payload: BatchTagRequest): Promise<BatchTagPrecheckResult[]>;
createBatchTags(payload: BatchTagRequest): Promise<BatchTagResult>;
```

其中 `createBatchTags` 包含后端预检查所需的配置快照，但真正执行前仍由后端重新验证 Tag 冲突和分支 head。

## 类型定义

```ts
export type RemoteTagType = 'lightweight' | 'annotated';

export interface RemoteBranch {
  name: string;
  isDefault: boolean;
}

export interface BranchHead {
  branch: string;
  sha: string;
  shortSha: string;
  subject: string;
}

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

export type BatchTagCheckCode =
  | 'ok'
  | 'repo_not_found'
  | 'permission_denied'
  | 'branch_not_found'
  | 'branch_head_unavailable'
  | 'tag_name_invalid'
  | 'tag_already_exists'
  | 'annotated_tag_unsupported'
  | 'annotation_required'
  | 'annotation_too_long'
  | 'duplicate_repo';

export interface BatchTagPrecheckResult {
  repoId: string;
  repoName: string;
  branch: string;
  head?: BranchHead;
  tagName: string;
  tagType: RemoteTagType;
  code: BatchTagCheckCode;
  message?: string;
  canCreate: boolean;
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
  errorCode?: BatchTagCheckCode | 'network' | 'rate_limited' | 'unknown';
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
```

## Tauri commands

### `list_remote_branches`

```text
Input:  { repoId: string }
Output: RemoteBranch[]
```

### `get_remote_branch_head`

```text
Input:  { repoId: string, branch: string }
Output: BranchHead
```

### `precheck_batch_tags`

```text
Input:  { payload: BatchTagRequest }
Output: BatchTagPrecheckResult[]
```

预检查为读操作，不创建 Tag，不写远程仓库。后端必须重新读取数据库中的仓库和账号，不接受前端传入的 owner、平台或 Token。

### `create_batch_tags`

```text
Input:  { payload: BatchTagRequest }
Output: BatchTagResult
```

后端规则：

1. 校验 `1 <= concurrency <= 20`；
2. 校验项目 ID 唯一且全部存在；
3. 逐项目读取仓库和对应账号 Provider；
4. 执行时重新读取分支 head 和 Tag 是否存在；
5. 按并发数执行，单项目错误转换为结果项，不提前终止整个批次；
6. 不覆盖已有 Tag；
7. 只取消尚未开始的项目；
8. 返回预检查 SHA 与实际 SHA；
9. 记录批量摘要及项目级日志。

## 参数校验

- `items` 不允许为空；
- `repoId` 不允许重复；
- `branch` 必须非空，且必须存在于目标远程仓库；
- `tagName` 使用 Git ref 校验器；
- `annotated` 时 `message` 必填，按 Unicode 字符计数 1～2000；
- `lightweight` 时忽略或拒绝非空 `message`，建议前端不发送；
- `concurrency` 只接受整数，范围 1～20。

## 兼容性约定

现有 `list_remote_branches` 当前返回 `string[]`，本特性建议升级为 `RemoteBranch[]`，以便统一标记默认分支；如果为降低影响暂时保留旧接口，应新增 `get_remote_branch_head` 使用相同仓库查询链路，并在前端适配两种返回结构。
