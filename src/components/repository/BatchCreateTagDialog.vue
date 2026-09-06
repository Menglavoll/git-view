<template>
  <ElDialog
    v-model="visible"
    title="批量添加 Tag"
    width="min(1180px, 94vw)"
    :close-on-click-modal="false"
    :close-on-press-escape="stage !== 'executing'"
    :show-close="stage !== 'executing'"
  >
    <template v-if="stage === 'editing' || stage === 'prechecking'">
      <div class="toolbar">
        <span>已选 {{ rows.length }} 个项目</span>
        <ElInputNumber
          v-model="concurrency"
          :min="1"
          :max="20"
          :precision="0"
          controls-position="right"
        />
        <span class="muted">并发数（1～20）</span>
        <ElInput v-model="unifiedTagName" placeholder="统一 Tag 名称" class="unified-input" />
        <ElButton :disabled="!unifiedTagName.trim()" @click="applyUnifiedName">应用到全部</ElButton>
        <ElButton @click="addDialogVisible = true">添加项目</ElButton>
      </div>

      <ElTable :data="rows" max-height="440" size="small">
        <ElTableColumn label="项目" min-width="180">
          <template #default="{ row }"
            ><span :title="row.repo.fullName">{{ row.repo.fullName }}</span></template
          >
        </ElTableColumn>
        <ElTableColumn label="平台" width="90">
          <template #default="{ row }">{{ platformName(row.repo.platform) }}</template>
        </ElTableColumn>
        <ElTableColumn label="账号" width="110"
          ><template #default="{ row }">{{
            accountName(row.repo.accountId)
          }}</template></ElTableColumn
        >
        <ElTableColumn label="分支 / 最新提交" min-width="240">
          <template #default="{ row }">
            <ElSelect
              v-model="row.branch"
              filterable
              :loading="branchLoading[row.repo.id]"
              @visible-change="(open: boolean) => open && loadBranches(row)"
              @change="loadHead(row)"
            >
              <ElOption
                v-for="branch in branchOptions[row.repo.id] ?? [row.repo.defaultBranch]"
                :key="branch"
                :label="branch"
                :value="branch"
              />
            </ElSelect>
            <div v-if="row.head" class="commit" :title="`${row.head.sha}\n${row.head.subject}`"
              >{{ row.head.shortSha }} · {{ row.head.subject || '无提交说明' }}</div
            >
            <div v-else-if="row.headError" class="error">{{ row.headError }}</div>
          </template>
        </ElTableColumn>
        <ElTableColumn label="类型" width="120">
          <template #default="{ row }"
            ><ElSelect v-model="row.tagType" @change="onTypeChange(row)"
              ><ElOption label="轻量 Tag" value="lightweight" /><ElOption
                label="附注 Tag"
                value="annotated" /></ElSelect
          ></template>
        </ElTableColumn>
        <ElTableColumn label="Tag 名称" min-width="160"
          ><template #default="{ row }"
            ><ElInput v-model="row.tagName" placeholder="例如 v1.2.0" /></template
        ></ElTableColumn>
        <ElTableColumn label="附注说明" min-width="220">
          <template #default="{ row }"
            ><ElInput
              v-if="row.tagType === 'annotated'"
              v-model="row.message"
              type="textarea"
              :rows="2"
              maxlength="2000"
              show-word-limit
              placeholder="附注 Tag 说明（1～2000 字符）"
            /><span v-else class="muted">—</span></template
          >
        </ElTableColumn>
        <ElTableColumn label="操作" width="72" fixed="right"
          ><template #default="{ row }"
            ><ElButton link type="danger" @click="removeRow(row.repo.id)">移除</ElButton></template
          ></ElTableColumn
        >
      </ElTable>
      <p v-if="rows.length === 0" class="error">至少保留一个项目。</p>
      <p v-if="validationMessage" class="error">{{ validationMessage }}</p>
    </template>

    <template v-else-if="stage === 'executing'">
      <div class="result-summary"
        >正在创建：已完成 {{ progress.completed }}/{{ progress.total }}，成功
        {{ progress.success }}，失败 {{ progress.failed }}，取消 {{ progress.cancelled }}</div
      >
      <p class="muted">已提交到远程平台的请求无法撤销；取消只会停止尚未开始的项目。</p>
      <ElTable :data="executionRows" max-height="420" size="small">
        <ElTableColumn prop="repo.fullName" label="项目" min-width="180"
          ><template #default="{ row }">{{ row.repo.fullName }}</template></ElTableColumn
        >
        <ElTableColumn prop="branch" label="分支" width="140" />
        <ElTableColumn prop="tagName" label="Tag" width="160" />
        <ElTableColumn label="状态" width="100">
          <template #default="{ row }">
            <ElTag :type="executionTagType(row.executionStatus)">{{
              executionStatusText(row.executionStatus)
            }}</ElTag>
          </template>
        </ElTableColumn>
        <ElTableColumn label="说明" min-width="240">
          <template #default="{ row }">{{ row.executionResult?.errorMessage ?? '—' }}</template>
        </ElTableColumn>
      </ElTable>
    </template>

    <template v-else-if="stage === 'result' && result">
      <div class="result-summary"
        >完成：成功 {{ result.success }}，失败 {{ result.failed }}，取消 {{ result.cancelled }}</div
      >
      <ElTable :data="result.items" max-height="420" size="small">
        <ElTableColumn prop="repoName" label="项目" min-width="180" />
        <ElTableColumn label="平台" width="90"
          ><template #default="{ row }">{{
            platformForResult(row.repoId)
          }}</template></ElTableColumn
        >
        <ElTableColumn prop="branch" label="分支" width="130" />
        <ElTableColumn prop="tagName" label="Tag" width="150" />
        <ElTableColumn label="提交" min-width="150"
          ><template #default="{ row }"
            ><span :title="row.actualSha">{{ row.actualSha?.slice(0, 7) ?? '—' }}</span
            ><span
              v-if="row.precheckedSha && row.actualSha && row.precheckedSha !== row.actualSha"
              class="warning"
            >
              已变化</span
            ></template
          ></ElTableColumn
        >
        <ElTableColumn label="状态" width="88"
          ><template #default="{ row }"
            ><ElTag
              :type="
                row.status === 'success'
                  ? 'success'
                  : row.status === 'cancelled'
                    ? 'info'
                    : 'danger'
              "
              >{{
                row.status === 'success' ? '成功' : row.status === 'cancelled' ? '已取消' : '失败'
              }}</ElTag
            ></template
          ></ElTableColumn
        >
        <ElTableColumn label="原因" min-width="220"
          ><template #default="{ row }">{{ row.errorMessage ?? '—' }}</template></ElTableColumn
        >
        <ElTableColumn label="耗时" width="88"
          ><template #default="{ row }">{{
            formatDuration(row.durationMs)
          }}</template></ElTableColumn
        >
      </ElTable>
    </template>

    <template #footer>
      <ElButton v-if="stage !== 'executing'" @click="visible = false">关闭</ElButton>
      <ElButton v-else type="warning" :loading="cancelling" :disabled="cancelling" @click="cancel"
        >取消剩余任务</ElButton
      >
      <ElButton v-if="stage === 'result' && result?.failed" type="warning" @click="retryFailures"
        >重试失败项目</ElButton
      >
      <ElButton
        v-if="stage === 'editing' || stage === 'prechecking'"
        type="primary"
        :loading="stage === 'prechecking'"
        :disabled="!canPrecheck"
        @click="runPrecheck"
        >执行预检查</ElButton
      >
    </template>
  </ElDialog>

  <ElDialog
    v-model="precheckDialogVisible"
    title="预检查结果"
    width="min(900px, 92vw)"
    :close-on-click-modal="false"
  >
    <ElTable :data="prechecks" max-height="390" size="small">
      <ElTableColumn prop="repoName" label="项目" min-width="180" />
      <ElTableColumn prop="accountName" label="账号" width="120" />
      <ElTableColumn prop="branch" label="分支" width="140" />
      <ElTableColumn label="预检查提交" width="120"
        ><template #default="{ row }"
          ><span :title="row.head?.sha">{{ row.head?.shortSha ?? '—' }}</span></template
        ></ElTableColumn
      >
      <ElTableColumn label="结果" width="100"
        ><template #default="{ row }"
          ><ElTag :type="row.canCreate ? 'success' : 'danger'">{{
            row.canCreate ? '可创建' : '需修正'
          }}</ElTag></template
        ></ElTableColumn
      >
      <ElTableColumn label="说明" min-width="220"
        ><template #default="{ row }">{{
          row.message ?? row.head?.subject ?? '—'
        }}</template></ElTableColumn
      >
    </ElTable>
    <p v-if="hasPrecheckFailure" class="error"
      >存在预检查失败项目。请关闭此窗口后修正配置或移除失败项目。</p
    >
    <template #footer
      ><ElButton @click="precheckDialogVisible = false">返回修改</ElButton
      ><ElButton type="primary" :disabled="hasPrecheckFailure" @click="openConfirm"
        >进入确认</ElButton
      ></template
    >
  </ElDialog>

  <ElDialog
    v-model="confirmDialogVisible"
    title="确认批量创建 Tag"
    width="min(960px, 92vw)"
    :close-on-click-modal="false"
  >
    <p
      >共 {{ rows.length }} 个项目，并发数 {{ concurrency }}。确认后将按执行时分支最新提交创建
      Tag。</p
    >
    <ElTable :data="rows" max-height="380" size="small"
      ><ElTableColumn label="项目" min-width="170"
        ><template #default="{ row }">{{ row.repo.fullName }}</template></ElTableColumn
      ><ElTableColumn label="平台" width="90"
        ><template #default="{ row }">{{
          platformName(row.repo.platform)
        }}</template></ElTableColumn
      ><ElTableColumn label="账号" width="110"
        ><template #default="{ row }">{{
          accountName(row.repo.accountId)
        }}</template></ElTableColumn
      ><ElTableColumn prop="branch" label="分支" width="130" /><ElTableColumn
        label="预检查 SHA"
        width="120"
        ><template #default="{ row }"
          ><span :title="precheckFor(row.repo.id)?.head?.sha">{{
            precheckFor(row.repo.id)?.head?.shortSha ?? '—'
          }}</span></template
        ></ElTableColumn
      ><ElTableColumn prop="tagName" label="Tag" width="150" /><ElTableColumn
        label="类型"
        width="100"
        ><template #default="{ row }">{{
          row.tagType === 'annotated' ? '附注' : '轻量'
        }}</template></ElTableColumn
      ><ElTableColumn label="附注说明" min-width="180"
        ><template #default="{ row }"
          ><div
            v-if="row.tagType === 'annotated'"
            class="annotation-summary"
            :title="row.message"
            >{{ row.message }}</div
          ><span v-else class="muted">—</span></template
        ></ElTableColumn
      ></ElTable
    >
    <template #footer
      ><ElButton @click="confirmDialogVisible = false">返回修改</ElButton
      ><ElButton type="primary" :loading="executing" @click="execute">确认创建</ElButton></template
    >
  </ElDialog>

  <ElDialog v-model="addDialogVisible" title="添加远程项目" width="760px"
    ><div class="add-toolbar"
      ><ElInput v-model="addKeyword" clearable placeholder="搜索项目" /><ElButton
        @click="emit('load-all')"
        >清除筛选并加载全部</ElButton
      ></div
    ><ElTable
      ref="addTable"
      :data="filteredAvailable"
      max-height="360"
      row-key="id"
      @selection-change="addSelection = $event"
      ><ElTableColumn type="selection" width="48" reserve-selection /><ElTableColumn
        prop="fullName"
        label="项目"
        min-width="220" /><ElTableColumn prop="platform" label="平台" width="100" /><ElTableColumn
        prop="defaultBranch"
        label="默认分支"
        width="130" /></ElTable
    ><template #footer
      ><ElButton @click="addDialogVisible = false">取消</ElButton
      ><ElButton type="primary" @click="addSelected">添加选中项目</ElButton></template
    ></ElDialog
  >
</template>

<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue';
import { ElMessage } from 'element-plus';
import type { UnlistenFn } from '@tauri-apps/api/event';

import { remoteRepositoryApi } from '@/api/remoteRepository.api';
import type { Account } from '@/types/account';
import type { RemoteRepository } from '@/types/repository';
import type {
  BatchTagItem,
  BatchTagExecutionStatus,
  BatchTagFinishedPayload,
  BatchTagPrecheckResult,
  BatchTagProgressPayload,
  BatchTagResult,
  BranchHead,
  RemoteTagType,
} from '@/types/tag';

interface Row {
  repo: RemoteRepository;
  branch: string;
  tagName: string;
  tagType: RemoteTagType;
  message?: string;
  head?: BranchHead;
  headError?: string;
  executionStatus?: BatchTagExecutionStatus;
  executionResult?: BatchTagResult['items'][number];
}
const props = defineProps<{
  modelValue: boolean;
  selectedRepos: RemoteRepository[];
  availableRepos: RemoteRepository[];
  accounts: Account[];
}>();
const emit = defineEmits<{
  (e: 'update:modelValue', value: boolean): void;
  (e: 'load-all'): void;
}>();
const visible = computed({
  get: () => props.modelValue,
  set: (value) => emit('update:modelValue', value),
});
const stage = ref<'editing' | 'prechecking' | 'executing' | 'result'>('editing');
const rows = ref<Row[]>([]);
const concurrency = ref(3);
const unifiedTagName = ref('');
const branchOptions = ref<Record<string, string[]>>({});
const branchLoading = ref<Record<string, boolean>>({});
const prechecks = ref<BatchTagPrecheckResult[]>([]);
const result = ref<BatchTagResult>();
const executing = ref(false);
const cancelling = ref(false);
const activeBatchId = ref<string>();
const progress = ref({ total: 0, completed: 0, success: 0, failed: 0, cancelled: 0 });
let unlistenProgress: UnlistenFn | undefined;
let unlistenFinished: UnlistenFn | undefined;
const precheckDialogVisible = ref(false);
const confirmDialogVisible = ref(false);
const addDialogVisible = ref(false);
const addKeyword = ref('');
const addSelection = ref<RemoteRepository[]>([]);
const filteredAvailable = computed(() => {
  const key = addKeyword.value.trim().toLowerCase();
  return props.availableRepos.filter((repo) => !key || repo.fullName.toLowerCase().includes(key));
});
const accountName = (id: string) =>
  props.accounts.find((account) => account.id === id)?.username ?? '—';
const platformName = (platform: RemoteRepository['platform']) =>
  ({ github: 'GitHub', gitlab: 'GitLab', gitee: 'Gitee' })[platform];
const platformForResult = (repoId: string) =>
  platformName(rows.value.find((row) => row.repo.id === repoId)?.repo.platform ?? 'github');
const precheckFor = (repoId: string) => prechecks.value.find((item) => item.repoId === repoId);
const formatDuration = (durationMs: number) =>
  durationMs < 1000 ? `${durationMs} ms` : `${(durationMs / 1000).toFixed(1)} 秒`;
const requestItems = computed<BatchTagItem[]>(() =>
  rows.value.map((row) => ({
    repoId: row.repo.id,
    branch: row.branch,
    tagName: row.tagName.trim(),
    tagType: row.tagType,
    message: row.tagType === 'annotated' ? row.message : undefined,
  })),
);
const validationMessage = computed(() => validateRows());
const canPrecheck = computed(
  () => rows.value.length > 0 && !validationMessage.value && stage.value === 'editing',
);
const hasPrecheckFailure = computed(() => prechecks.value.some((item) => !item.canCreate));
const executionRows = computed(() => rows.value);
watch(
  () => props.modelValue,
  (open) => {
    if (open) reset(props.selectedRepos);
  },
);
onMounted(async () => {
  unlistenProgress = await remoteRepositoryApi.onBatchTagProgress(handleProgress);
  unlistenFinished = await remoteRepositoryApi.onBatchTagFinished(handleFinished);
});
onBeforeUnmount(() => {
  unlistenProgress?.();
  unlistenFinished?.();
});
function reset(repos: RemoteRepository[]): void {
  stage.value = 'editing';
  result.value = undefined;
  prechecks.value = [];
  activeBatchId.value = undefined;
  progress.value = { total: 0, completed: 0, success: 0, failed: 0, cancelled: 0 };
  cancelling.value = false;
  unifiedTagName.value = '';
  concurrency.value = 3;
  branchOptions.value = {};
  branchLoading.value = {};
  rows.value = repos.map(makeRow);
}
function makeRow(repo: RemoteRepository): Row {
  return { repo, branch: repo.defaultBranch, tagName: '', tagType: 'lightweight' };
}
function validateRows(): string | null {
  if (!Number.isInteger(concurrency.value) || concurrency.value < 1 || concurrency.value > 20)
    return '并发数必须是 1～20 的整数。';
  for (const row of rows.value) {
    if (!row.branch) return `${row.repo.fullName} 未选择分支。`;
    if (!validTag(row.tagName)) return `${row.repo.fullName} 的 Tag 名称不合法。`;
    const length = (row.message ?? '').length;
    if (row.tagType === 'annotated' && (length < 1 || length > 2000))
      return `${row.repo.fullName} 的附注说明必须为 1～2000 字符。`;
  }
  return null;
}
function validTag(name: string): boolean {
  const value = name.trim();
  return (
    !!value &&
    value !== '.' &&
    value !== '..' &&
    !Array.from(value).some(
      (character) => /\s/.test(character) || '~^:?*[\\'.includes(character),
    ) &&
    !value.startsWith('/') &&
    !value.endsWith('/') &&
    !value.includes('..') &&
    !value.includes('@{') &&
    !value.endsWith('.lock')
  );
}
function applyUnifiedName(): void {
  rows.value.forEach((row) => {
    row.tagName = unifiedTagName.value.trim();
  });
}
function onTypeChange(row: Row): void {
  if (row.tagType === 'lightweight') row.message = undefined;
}
function removeRow(repoId: string): void {
  rows.value = rows.value.filter((row) => row.repo.id !== repoId);
}
async function loadBranches(row: Row): Promise<void> {
  if (branchOptions.value[row.repo.id] || branchLoading.value[row.repo.id]) return;
  branchLoading.value[row.repo.id] = true;
  try {
    branchOptions.value[row.repo.id] = await remoteRepositoryApi.listBranches(row.repo.id);
    await loadHead(row);
  } catch (error) {
    row.headError = `加载分支失败：${error instanceof Error ? error.message : String(error)}`;
  } finally {
    branchLoading.value[row.repo.id] = false;
  }
}
async function loadHead(row: Row): Promise<void> {
  row.head = undefined;
  row.headError = undefined;
  try {
    row.head = await remoteRepositoryApi.getBranchHead(row.repo.id, row.branch);
  } catch (error) {
    row.headError = `读取提交失败：${error instanceof Error ? error.message : String(error)}`;
  }
}
async function runPrecheck(): Promise<void> {
  if (!canPrecheck.value) return;
  stage.value = 'prechecking';
  try {
    prechecks.value = await remoteRepositoryApi.precheckBatchTags({
      items: requestItems.value,
      concurrency: concurrency.value,
    });
    prechecks.value.forEach((check) => {
      const row = rows.value.find((item) => item.repo.id === check.repoId);
      if (row && check.head) row.head = check.head;
    });
    precheckDialogVisible.value = true;
  } catch (error) {
    ElMessage.error(`预检查失败：${error instanceof Error ? error.message : String(error)}`);
  } finally {
    stage.value = 'editing';
  }
}
function openConfirm(): void {
  precheckDialogVisible.value = false;
  confirmDialogVisible.value = true;
}
async function execute(): Promise<void> {
  executing.value = true;
  try {
    const started = await remoteRepositoryApi.startBatchTags(
      { items: requestItems.value, concurrency: concurrency.value },
      prechecks.value,
    );
    activeBatchId.value = started.batchId;
    progress.value = {
      total: rows.value.length,
      completed: 0,
      success: 0,
      failed: 0,
      cancelled: 0,
    };
    rows.value.forEach((row) => {
      row.executionStatus = undefined;
      row.executionResult = undefined;
    });
    confirmDialogVisible.value = false;
    stage.value = 'executing';
  } catch (error) {
    ElMessage.error(`创建 Tag 失败：${error instanceof Error ? error.message : String(error)}`);
  } finally {
    executing.value = false;
  }
}
function handleProgress(payload: BatchTagProgressPayload): void {
  if (payload.batchId !== activeBatchId.value) return;
  progress.value = {
    total: payload.total,
    completed: payload.completed,
    success: payload.success,
    failed: payload.failed,
    cancelled: payload.cancelled,
  };
  const row = rows.value.find((item) => item.repo.id === payload.repoId);
  if (row) {
    row.executionStatus = payload.status;
    if (payload.result) row.executionResult = payload.result;
  }
}
function handleFinished(payload: BatchTagFinishedPayload): void {
  if (payload.batchId !== activeBatchId.value) return;
  result.value = payload.result;
  payload.result.items.forEach((item) => {
    const row = rows.value.find((candidate) => candidate.repo.id === item.repoId);
    if (row) {
      row.executionStatus = item.status;
      row.executionResult = item;
    }
  });
  activeBatchId.value = undefined;
  cancelling.value = false;
  stage.value = 'result';
}
async function cancel(): Promise<void> {
  if (!activeBatchId.value || cancelling.value) return;
  cancelling.value = true;
  try {
    await remoteRepositoryApi.cancelBatchTags(activeBatchId.value);
    ElMessage.info('已请求取消尚未开始的项目，正在执行的请求会继续完成。');
  } catch (error) {
    ElMessage.error(`取消失败：${error instanceof Error ? error.message : String(error)}`);
    cancelling.value = false;
  }
}
function executionStatusText(status?: BatchTagExecutionStatus): string {
  switch (status) {
    case 'running':
      return '执行中';
    case 'success':
      return '成功';
    case 'failed':
      return '失败';
    case 'cancelled':
      return '已取消';
    default:
      return '等待中';
  }
}
function executionTagType(
  status?: BatchTagExecutionStatus,
): 'success' | 'danger' | 'info' | 'warning' {
  if (status === 'success') return 'success';
  if (status === 'failed') return 'danger';
  if (status === 'cancelled') return 'info';
  return 'warning';
}
function retryFailures(): void {
  const failed = new Set(
    result.value?.items.filter((item) => item.status === 'failed').map((item) => item.repoId),
  );
  rows.value = rows.value.filter((row) => failed.has(row.repo.id));
  result.value = undefined;
  prechecks.value = [];
  stage.value = 'editing';
}
function addSelected(): void {
  const exists = new Set(rows.value.map((row) => row.repo.id));
  rows.value.push(...addSelection.value.filter((repo) => !exists.has(repo.id)).map(makeRow));
  addDialogVisible.value = false;
  addSelection.value = [];
}
</script>

<style scoped>
.toolbar,
.add-toolbar {
  display: flex;
  align-items: center;
  gap: 8px;
  margin-bottom: 12px;
  flex-wrap: wrap;
}
.unified-input {
  width: 180px;
}
.muted {
  color: var(--el-text-color-secondary);
  font-size: 12px;
}
.commit {
  font-size: 12px;
  color: var(--el-text-color-secondary);
  margin-top: 3px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.error {
  color: var(--el-color-danger);
  font-size: 12px;
  margin: 8px 0;
}
.warning {
  color: var(--el-color-warning);
}
.result-summary {
  margin-bottom: 12px;
  font-weight: 600;
}
.annotation-summary {
  display: -webkit-box;
  overflow: hidden;
  -webkit-box-orient: vertical;
  -webkit-line-clamp: 2;
  white-space: pre-wrap;
}
</style>
