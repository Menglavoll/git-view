<template>
  <div class="page-remote-repositories">
    <div class="page-header">
      <h2 class="page-title">远程仓库</h2>
      <div class="header-actions">
        <ElRadioGroup v-model="viewMode">
          <ElRadioButton value="tree">树形</ElRadioButton>
          <ElRadioButton value="list">列表</ElRadioButton>
        </ElRadioGroup>
        <ElButton type="primary" :disabled="selection.length === 0" @click="onBatchClone">
          批量 Clone ({{ selection.length }})
        </ElButton>
        <ElButton type="success" :disabled="selection.length === 0" @click="onBatchCreateTag">
          批量添加 Tag ({{ selection.length }})
        </ElButton>
        <ElButton :loading="store.syncing" @click="onRefresh()">同步全部</ElButton>
      </div>
    </div>

    <div class="filter-bar">
      <ElInput
        v-model="searchText"
        placeholder="搜索仓库名或描述"
        clearable
        style="width: 240px"
        @input="onSearchDebounced"
      />
      <ElSelect
        v-model="filterPlatforms"
        multiple
        placeholder="平台"
        clearable
        collapse-tags
        style="width: 160px"
        @change="onFilterChange"
      >
        <ElOption label="GitHub" value="github" />
        <ElOption label="GitLab" value="gitlab" />
        <ElOption label="Gitee" value="gitee" />
      </ElSelect>
      <ElSelect
        v-model="filterAccountId"
        placeholder="账号"
        clearable
        style="width: 180px"
        @change="onFilterChange"
      >
        <ElOption
          v-for="acc in accountStore.accounts"
          :key="acc.id"
          :label="acc.username"
          :value="acc.id"
        />
      </ElSelect>
      <ElCheckbox v-model="filterFavorite" @change="onFilterChange">仅收藏</ElCheckbox>

      <span v-if="selection.length > 0" class="selection-info">
        已选 {{ selection.length }} 个
      </span>
    </div>

    <RemoteRepoTree
      v-if="viewMode === 'tree'"
      :items="store.repositories"
      :loading="store.loading"
      empty-text="暂无远程仓库，请先同步账号"
      @update:selection="selection = $event"
      @open-detail="onOpenDetail"
      @clone="onClone"
      @toggle-favorite="onToggleFavorite"
      @copy-url="onCopyFromTable"
    />
    <RemoteRepoTable
      v-else
      :items="store.repositories"
      :loading="store.loading"
      empty-text="暂无远程仓库，请先同步账号"
      @update:selection="selection = $event"
      @open-detail="onOpenDetail"
      @clone="onClone"
      @toggle-favorite="onToggleFavorite"
      @copy-url="onCopyFromTable"
    />

    <RepoDetailDrawer
      v-model="drawerVisible"
      :repo="activeRepo"
      @clone="onClone"
      @toggle-favorite="onToggleFavorite"
    />

    <BatchCloneDialog
      v-model="batchDialogVisible"
      :selected-repos="cloneTargets"
      @started="onCloneStarted"
    />
    <BatchCreateTagDialog
      v-model="tagDialogVisible"
      :selected-repos="tagTargets"
      :available-repos="store.repositories"
      :accounts="accountStore.accounts"
      @load-all="onLoadAllForTag"
    />
  </div>
</template>

<script setup lang="ts">
import { onMounted, ref, watch } from 'vue';
import { ElMessage } from 'element-plus';

import { useRouter } from 'vue-router';

import BatchCloneDialog from '@/components/clone/BatchCloneDialog.vue';
import BatchCreateTagDialog from '@/components/repository/BatchCreateTagDialog.vue';
import RemoteRepoTable from '@/components/repository/RemoteRepoTable.vue';
import RemoteRepoTree from '@/components/repository/RemoteRepoTree.vue';
import RepoDetailDrawer from '@/components/repository/RepoDetailDrawer.vue';
import { useRemoteRepositoryStore } from '@/stores/remoteRepository';
import { useAccountStore } from '@/stores/account';
import type { RemoteRepository } from '@/types/repository';

const store = useRemoteRepositoryStore();
const accountStore = useAccountStore();
const router = useRouter();

const viewMode = ref<'tree' | 'list'>('tree');
const searchText = ref('');
const filterPlatforms = ref<string[]>([]);
const filterAccountId = ref<string | undefined>(undefined);
const filterFavorite = ref(false);
const selection = ref<RemoteRepository[]>([]);
const drawerVisible = ref(false);
const activeRepo = ref<RemoteRepository | null>(null);
const batchDialogVisible = ref(false);
const cloneTargets = ref<RemoteRepository[]>([]);
const tagDialogVisible = ref(false);
const tagTargets = ref<RemoteRepository[]>([]);

let debounceTimer: ReturnType<typeof setTimeout> | null = null;

// 切换视图时清空选择：树/列表勾选状态相互独立，避免残留旧选中影响批量操作
watch(viewMode, () => {
  selection.value = [];
});

onMounted(() => {
  void accountStore.loadAccounts().catch(() => {});
  void store.fetchList().catch((e) => {
    ElMessage.error(`加载仓库失败：${e instanceof Error ? e.message : String(e)}`);
  });
});

function onSearchDebounced(): void {
  if (debounceTimer) clearTimeout(debounceTimer);
  debounceTimer = setTimeout(onFilterChange, 300);
}

function onFilterChange(): void {
  void store
    .fetchList({
      search: searchText.value || undefined,
      platforms: filterPlatforms.value.length > 0 ? filterPlatforms.value : undefined,
      accountId: filterAccountId.value,
      onlyFavorite: filterFavorite.value || undefined,
    })
    .catch(() => {});
}

async function onRefresh(): Promise<void> {
  try {
    const count = await store.refresh();
    ElMessage.success(`同步完成（${count} 个仓库）`);
  } catch (e) {
    ElMessage.error(`同步失败：${e instanceof Error ? e.message : String(e)}`);
  }
}

async function onToggleFavorite(row: RemoteRepository): Promise<void> {
  try {
    await store.toggleFavorite(row.id);
  } catch (e) {
    ElMessage.error(`操作失败：${e instanceof Error ? e.message : String(e)}`);
  }
}

function onOpenDetail(repo: RemoteRepository): void {
  activeRepo.value = repo;
  drawerVisible.value = true;
}

function onClone(repo: RemoteRepository): void {
  cloneTargets.value = [repo];
  batchDialogVisible.value = true;
}

function onBatchClone(): void {
  if (selection.value.length === 0) return;
  cloneTargets.value = [...selection.value];
  batchDialogVisible.value = true;
}

/** 复用当前树/列表的选择结果打开批量 Tag 配置页。 */
function onBatchCreateTag(): void {
  if (selection.value.length === 0) return;
  tagTargets.value = [...selection.value];
  tagDialogVisible.value = true;
}

/** 批量 Tag 的添加器允许清除页面筛选，重新提供全部已同步远程项目。 */
function onLoadAllForTag(): void {
  searchText.value = '';
  filterPlatforms.value = [];
  filterAccountId.value = undefined;
  filterFavorite.value = false;
  void store.fetchList({}).catch((e) => {
    ElMessage.error(`加载全部仓库失败：${e instanceof Error ? e.message : String(e)}`);
  });
}

function onCloneStarted(): void {
  void router.push({ name: 'clone-center' });
}

function onCopyFromTable(payload: { repo: RemoteRepository; type: 'https' | 'ssh' }): void {
  const url = payload.type === 'ssh' ? payload.repo.sshUrl : payload.repo.cloneUrl;
  if (!url) {
    ElMessage.warning('该仓库无对应协议地址');
    return;
  }
  void navigator.clipboard.writeText(url).then(() => {
    ElMessage.success('已复制到剪贴板');
  });
}
</script>

<style scoped>
.page-remote-repositories {
  padding: 8px;
}

.page-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  margin-bottom: 16px;
}

.page-title {
  margin: 0;
  font-size: 20px;
  font-weight: 600;
}

/* 头部操作区：横向排列并垂直居中，消除 radio 组与按钮的基线错位 */
.header-actions {
  display: flex;
  align-items: center;
  gap: 8px;
}

.filter-bar {
  display: flex;
  align-items: center;
  gap: 12px;
  margin-bottom: 16px;
  flex-wrap: wrap;
}

.selection-info {
  margin-left: auto;
  color: var(--el-color-primary);
  font-size: 13px;
}
</style>
