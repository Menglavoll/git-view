// =====================================================================
// 远程仓库 API 封装
// 封装与 src-tauri/src/commands/remote_repositories.rs 对应的 5 个 IPC 命令。
// =====================================================================

import { invokeCmd } from './tauri';
import type { CommitDetail, CommitPage } from '@/types/git';
import type { RemoteRepository } from '@/types/repository';
import type {
  BatchTagPrecheckResult,
  BatchTagRequest,
  BatchTagResult,
  BranchHead,
} from '@/types/tag';

/** 远程仓库筛选条件。 */
export interface RemoteRepoFilter {
  accountId?: string;
  platforms?: string[];
  owners?: string[];
  visibilities?: string[];
  onlyFavorite?: boolean;
  search?: string;
}

export const remoteRepositoryApi = {
  /** 查询远程仓库列表。 */
  list(filter: RemoteRepoFilter = {}): Promise<RemoteRepository[]> {
    return invokeCmd<RemoteRepository[]>('list_remote_repositories', { filter });
  },

  /** 搜索远程仓库。 */
  search(keyword: string, filter: RemoteRepoFilter = {}): Promise<RemoteRepository[]> {
    return invokeCmd<RemoteRepository[]>('search_remote_repositories', { keyword, filter });
  },

  /** 刷新远程仓库（触发同步）。 */
  refresh(accountId?: string): Promise<number> {
    return invokeCmd<number>('refresh_remote_repositories', { accountId: accountId ?? null });
  },

  /** 获取单个远程仓库详情。 */
  getDetail(repoId: string): Promise<RemoteRepository> {
    return invokeCmd<RemoteRepository>('get_remote_repository_detail', { repoId });
  },

  /** 切换收藏状态。 */
  toggleFavorite(repoId: string): Promise<boolean> {
    return invokeCmd<boolean>('toggle_favorite_remote_repository', { repoId });
  },

  /** 拉取远程仓库提交历史（page 从 1 起、缺省每页 30）。 */
  listCommits(repoId: string, page?: number, perPage?: number): Promise<CommitPage> {
    return invokeCmd<CommitPage>('list_remote_commits', {
      repoId,
      page: page ?? null,
      perPage: perPage ?? null,
    });
  },

  /** 获取远程仓库单个提交的详情。 */
  getCommitDetail(repoId: string, sha: string): Promise<CommitDetail> {
    return invokeCmd<CommitDetail>('get_remote_commit_detail', { repoId, sha });
  },

  /** 拉取远程仓库的分支列表（从平台 API，供克隆时选择分支）。 */
  listBranches(repoId: string): Promise<string[]> {
    return invokeCmd<string[]>('list_remote_branches', { repoId });
  },

  /** 读取远程分支最新提交，供批量 Tag 配置页展示 SHA 与提交说明。 */
  getBranchHead(repoId: string, branch: string): Promise<BranchHead> {
    return invokeCmd<BranchHead>('get_remote_branch_head', { repoId, branch });
  },

  /** 只读校验批量 Tag 配置；失败项目必须修正或移除后才能确认创建。 */
  precheckBatchTags(payload: BatchTagRequest): Promise<BatchTagPrecheckResult[]> {
    return invokeCmd<BatchTagPrecheckResult[]>('precheck_batch_tags', { payload });
  },

  /** 确认窗口后的实际远程创建；后端会重新检查分支 head 与同名 Tag。 */
  createBatchTags(
    payload: BatchTagRequest,
    prechecks: BatchTagPrecheckResult[],
  ): Promise<BatchTagResult> {
    return invokeCmd<BatchTagResult>('create_batch_tags', { payload, prechecks });
  },
};
