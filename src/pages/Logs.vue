<!-- 操作日志页面（T097 / US6）。 -->
<!-- 功能： -->
<!--   - 顶部工具栏：清理范围选择 + 清理日志按钮（破坏性，经 ConfirmDangerDialog 二次确认） -->
<!--   - 筛选区：操作类型多选、状态多选、关键字（匹配 target），查询 / 重置 -->
<!--   - 主体：el-table 展示当前页日志，状态/类型以 Tag 呈现，命令列溢出省略 -->
<!--   - 分页：上一页 / 下一页（后端不返回总数，以满页推断是否有下一页） -->
<!--   - 详情对话框：展示完整命令 / 输出 / 错误（均已脱敏）与中文错误翻译 -->
<!-- 约束： -->
<!--   - 日志在后端写入时已统一脱敏，前端仅展示，不再处理 token（SC-009/010） -->
<!--   - 清理日志属删除操作，必经 ConfirmDangerDialog（宪法 Principle III） -->
<!--   - 筛选维度严格对齐后端 LogFilter，不引入后端不支持的条件 -->
<template>
  <div class="page-logs">
    <!-- 顶部标题与清理入口 -->
    <div class="page-header">
      <h2 class="page-title">操作日志</h2>
      <div class="header-actions">
        <!-- 清理范围：全部 / 指定天数前；驱动 clearOld 的 beforeDays 参数 -->
        <el-select v-model="clearBeforeDays" class="clear-range" placeholder="清理范围">
          <el-option :value="'all'" label="清空全部" />
          <el-option :value="30" label="30 天前" />
          <el-option :value="90" label="90 天前" />
        </el-select>
        <!-- 清理按钮：仅打开二次确认对话框，真正删除在 onConfirmClear -->
        <el-button type="danger" plain :loading="store.clearing" @click="onClickClear">
          清理日志
        </el-button>
      </div>
    </div>

    <!-- 筛选条件区：均直接绑定 store 的筛选 ref，查询时统一组装 LogFilter -->
    <div class="filter-bar">
      <!-- 操作类型多选：空表示不限类型 -->
      <el-select
        v-model="store.operationTypes"
        multiple
        collapse-tags
        clearable
        placeholder="操作类型"
        class="filter-select"
      >
        <el-option
          v-for="opt in typeOptions"
          :key="opt.value"
          :label="opt.label"
          :value="opt.value"
        />
      </el-select>
      <!-- 状态多选：空表示不限状态 -->
      <el-select
        v-model="store.statuses"
        multiple
        collapse-tags
        clearable
        placeholder="状态"
        class="filter-select"
      >
        <el-option
          v-for="opt in statusOptions"
          :key="opt.value"
          :label="opt.label"
          :value="opt.value"
        />
      </el-select>
      <!-- 关键字：模糊匹配 target（仓库名 / 账号名） -->
      <el-input
        v-model="store.keyword"
        clearable
        placeholder="关键字（目标）"
        class="filter-keyword"
        @keyup.enter="onSearch"
      />
      <!-- 查询：回到第一页并应用筛选 -->
      <el-button type="primary" :loading="store.loading" @click="onSearch">查询</el-button>
      <!-- 重置：清空全部筛选并刷新 -->
      <el-button @click="onReset">重置</el-button>
    </div>

    <!-- 错误提示条：仅在最近一次操作出错时显示（错误已脱敏） -->
    <el-alert
      v-if="store.error"
      class="error-bar"
      type="error"
      :closable="false"
      :title="store.error"
    />

    <!-- 日志表格：所有字段后端已脱敏，命令/目标列溢出以 tooltip 展示 -->
    <el-table
      v-loading="store.loading"
      :data="store.logs"
      empty-text="暂无操作日志"
      class="log-table"
    >
      <!-- 发生时间：本地化展示 -->
      <el-table-column label="时间" width="180">
        <template #default="{ row }">{{ formatTime(row.occurredAt) }}</template>
      </el-table-column>
      <!-- 操作类型：中文 Tag -->
      <el-table-column label="操作" width="110">
        <template #default="{ row }">
          <el-tag size="small">{{ typeLabel(row.operationType) }}</el-tag>
        </template>
      </el-table-column>
      <!-- 操作目标：可能较长，溢出省略 -->
      <el-table-column label="目标" prop="target" min-width="180" show-overflow-tooltip />
      <!-- 结果状态：成功绿 / 失败红 / 取消灰 -->
      <el-table-column label="状态" width="90">
        <template #default="{ row }">
          <el-tag size="small" :type="statusMeta(row.status).tagType">
            {{ statusMeta(row.status).label }}
          </el-tag>
        </template>
      </el-table-column>
      <!-- 耗时：毫秒 -->
      <el-table-column label="耗时" width="90">
        <template #default="{ row }">{{ row.durationMs }} ms</template>
      </el-table-column>
      <!-- 命令摘要：溢出省略，完整内容在详情查看 -->
      <el-table-column label="命令" prop="command" min-width="200" show-overflow-tooltip />
      <!-- 操作列：查看详情 -->
      <el-table-column label="" width="80" align="right">
        <template #default="{ row }">
          <el-button link type="primary" @click="onShowDetail(row)">详情</el-button>
        </template>
      </el-table-column>
    </el-table>

    <!-- 分页：后端不返回总数，仅提供上一页 / 下一页 -->
    <div class="pager">
      <el-button :disabled="!store.hasPrevPage || store.loading" @click="onPrev">上一页</el-button>
      <span class="pager-info">第 {{ store.page + 1 }} 页</span>
      <el-button :disabled="!store.hasNextPage || store.loading" @click="onNext">下一页</el-button>
    </div>

    <!-- 日志详情对话框：展示完整脱敏后的命令 / 输出 / 错误与中文翻译 -->
    <el-dialog v-model="detailVisible" title="日志详情" width="640px">
      <div v-if="currentDetail" class="detail">
        <!-- 概要信息：类型 / 状态 / 目标 / 时间 / 耗时 -->
        <div class="detail-meta">
          <span class="detail-label">操作</span>
          <el-tag size="small">{{ typeLabel(currentDetail.operationType) }}</el-tag>
          <el-tag size="small" :type="statusMeta(currentDetail.status).tagType">
            {{ statusMeta(currentDetail.status).label }}
          </el-tag>
          <span class="detail-time">{{ formatTime(currentDetail.occurredAt) }}</span>
          <span class="detail-time">{{ currentDetail.durationMs }} ms</span>
        </div>
        <!-- 目标 -->
        <div class="detail-row">
          <span class="detail-label">目标</span>
          <span class="detail-value">{{ currentDetail.target }}</span>
        </div>
        <!-- 命令（已脱敏） -->
        <div v-if="currentDetail.command" class="detail-row">
          <span class="detail-label">命令</span>
          <pre class="detail-pre">{{ currentDetail.command }}</pre>
        </div>
        <!-- 输出摘要（已脱敏） -->
        <div v-if="currentDetail.output" class="detail-row">
          <span class="detail-label">输出</span>
          <pre class="detail-pre">{{ currentDetail.output }}</pre>
        </div>
        <!-- 错误信息（已脱敏，红色） -->
        <div v-if="currentDetail.errorMessage" class="detail-row">
          <span class="detail-label">错误</span>
          <pre class="detail-pre detail-error">{{ currentDetail.errorMessage }}</pre>
        </div>
        <!-- 中文错误翻译：来自后端 translate_error，便于用户理解 -->
        <div v-if="currentDetail.translatedErrorMessage" class="detail-row">
          <span class="detail-label">提示</span>
          <p class="detail-hint">{{ currentDetail.translatedErrorMessage }}</p>
        </div>
      </div>
      <template #footer>
        <el-button @click="detailVisible = false">关闭</el-button>
      </template>
    </el-dialog>

    <!-- 清理日志二次确认：beforeDays 决定清空全部还是清理 N 天前 -->
    <ConfirmDangerDialog
      v-model:visible="clearVisible"
      title="清理操作日志"
      :message="clearMessage"
      recoverability-hint="清理后的日志无法恢复；已克隆仓库与账号不受影响。"
      confirm-button-text="确认清理"
      :loading="store.clearing"
      @confirm="onConfirmClear"
    />
  </div>
</template>

<script setup lang="ts">
// =====================================================================
// 操作日志页面脚本（T097 / US6）。
// 职责：
//   - 组合筛选区、表格、分页、详情对话框，承接用户操作并调用 store action
//   - 处理清理日志的二次确认（ConfirmDangerDialog）与消息提示
//   - 提供操作类型 / 状态的中文标签映射（展示层逻辑）
// 注意：日志数据在后端写入时已脱敏，本页面仅做展示，不接触原始 token。
// =====================================================================
import { computed, onMounted, ref } from 'vue'; // 组合式 API：响应式 + 生命周期
import { ElMessage } from 'element-plus'; // 轻量消息提示

import ConfirmDangerDialog from '@/components/common/ConfirmDangerDialog.vue'; // 危险操作确认
import { useOperationLogStore } from '@/stores/operationLog'; // 日志 store
import type { OperationLog, OperationStatus, OperationType } from '@/types/operationLog'; // 类型

// 日志 store：筛选条件、列表、分页与 action 均从此获取
const store = useOperationLogStore();

// ---------------------------------------------------------------------
// 展示映射：操作类型 / 状态 → 中文标签（与后端枚举一一对应）。
// 前端自维护这份映射而非依赖后端返回：枚举值是稳定契约，中文文案属纯展示层，
// 改文案无需触碰后端，也便于将来做多语言。
// ---------------------------------------------------------------------

/** 操作类型 → 中文标签 */
const OPERATION_TYPE_LABELS: Record<OperationType, string> = {
  add_account: '添加账号',
  delete_account: '删除账号',
  test_connection: '测试连接',
  sync_repos: '同步仓库',
  clone: '克隆',
  fetch: 'Fetch',
  pull: 'Pull',
  push: 'Push',
  commit: '提交',
  checkout: '切换分支',
  create_branch: '新建分支',
  batch_create_tag: '批量创建 Tag',
  create_tag: '创建 Tag',
  scan_repos: '扫描仓库',
  discard_changes: '丢弃变更',
};

/** 状态 → 中文标签 + el-tag 颜色类型 */
const STATUS_META: Record<
  OperationStatus,
  { label: string; tagType: 'success' | 'danger' | 'info' }
> = {
  success: { label: '成功', tagType: 'success' },
  failed: { label: '失败', tagType: 'danger' },
  cancelled: { label: '已取消', tagType: 'info' },
};

/** 类型下拉选项（由标签映射生成，避免重复维护） */
const typeOptions = (Object.keys(OPERATION_TYPE_LABELS) as OperationType[]).map((value) => ({
  value,
  label: OPERATION_TYPE_LABELS[value],
}));

/** 状态下拉选项 */
const statusOptions = (Object.keys(STATUS_META) as OperationStatus[]).map((value) => ({
  value,
  label: STATUS_META[value].label,
}));

/** 取操作类型中文标签（未知值回退原值，理论上不发生）。 */
function typeLabel(t: OperationType): string {
  // ?? 回退原值：后端枚举与此表本应同步，回退仅防御未来新增类型时漏更映射
  return OPERATION_TYPE_LABELS[t] ?? t;
}

/** 取状态展示元信息（未知值回退为灰色 info）。 */
function statusMeta(s: OperationStatus): { label: string; tagType: 'success' | 'danger' | 'info' } {
  // 未知状态回退中性灰：不认识的状态既不标成功也不标失败，避免误导
  return STATUS_META[s] ?? { label: s, tagType: 'info' };
}

/** 把 ISO 时间格式化为本地可读字符串；解析失败时回退原串。 */
function formatTime(iso: string): string {
  const d = new Date(iso);
  // 解析失败回退原始 ISO 串：宁可展示原文，也不显示 "Invalid Date" 误导用户
  return Number.isNaN(d.getTime()) ? iso : d.toLocaleString();
}

// ---------------------------------------------------------------------
// 筛选与分页动作
// ---------------------------------------------------------------------

/** 查询：回到第一页并应用当前筛选。 */
async function onSearch(): Promise<void> {
  try {
    // applyFilters 内部会重置到第一页：避免在高页码上套用新筛选而查到空结果
    await store.applyFilters();
  } catch (e) {
    ElMessage.error(`查询日志失败：${e instanceof Error ? e.message : String(e)}`);
  }
}

/** 重置：清空全部筛选并刷新。 */
async function onReset(): Promise<void> {
  try {
    // resetFilters 清空类型/状态/关键字并回到首页，再触发一次查询
    await store.resetFilters();
  } catch (e) {
    ElMessage.error(`重置失败：${e instanceof Error ? e.message : String(e)}`);
  }
}

/** 上一页。 */
async function onPrev(): Promise<void> {
  try {
    // 后端不返回总数，翻页由 store 维护 page 游标 + hasPrev/hasNext 推断
    await store.prevPage();
  } catch (e) {
    ElMessage.error(`翻页失败：${e instanceof Error ? e.message : String(e)}`);
  }
}

/** 下一页。 */
async function onNext(): Promise<void> {
  try {
    // 满页才认为可能有下一页：hasNextPage 由 store 按返回条数是否等于 pageSize 推断
    await store.nextPage();
  } catch (e) {
    ElMessage.error(`翻页失败：${e instanceof Error ? e.message : String(e)}`);
  }
}

// ---------------------------------------------------------------------
// 详情对话框
// 详情数据可能比列表更新，故点开时重新向后端拉取单条记录
// ---------------------------------------------------------------------

/** 详情对话框可见性 */
const detailVisible = ref(false);
/** 当前查看的日志详情 */
const currentDetail = ref<OperationLog | null>(null);

/** 查看某行详情：请求后端获取最新单条，失败时回退用行内数据。 */
async function onShowDetail(row: OperationLog): Promise<void> {
  try {
    // 重新拉单条而非直接用 row：列表可能是旧数据，详情尽量取最新
    const detail = await store.getDetail(row.id);
    currentDetail.value = detail ?? row;
  } catch {
    // 详情查询失败不阻塞：直接用列表行数据展示
    currentDetail.value = row;
  }
  detailVisible.value = true;
}

// ---------------------------------------------------------------------
// 清理日志（破坏性，二次确认）
// 清理不可恢复，必须经 ConfirmDangerDialog 二次确认（宪法 Principle III）
// ---------------------------------------------------------------------

/** 清理确认对话框可见性 */
const clearVisible = ref(false);
/** 清理范围：'all' 表示清空全部，数字表示删除 N 天前 */
const clearBeforeDays = ref<number | 'all'>('all');

/** 根据清理范围动态生成确认文案。 */
// 文案随范围动态变化：让用户在确认框里清楚看到要删的是「全部」还是「N 天前」
const clearMessage = computed(() =>
  clearBeforeDays.value === 'all'
    ? '确认清空全部操作日志吗？所有历史记录将被永久删除。'
    : `确认删除 ${clearBeforeDays.value} 天前的操作日志吗？该范围内的记录将被永久删除。`,
);

/** 点击「清理日志」：仅打开二次确认对话框。 */
function onClickClear(): void {
  clearVisible.value = true;
}

/** 二次确认通过后真正执行清理。 */
async function onConfirmClear(): Promise<void> {
  // 走到这里说明用户已在 ConfirmDangerDialog 点了确认，可以真正删除
  try {
    // 'all' 映射为 undefined（清空全部），数字直接作为「N 天前」阈值
    const days = clearBeforeDays.value === 'all' ? undefined : clearBeforeDays.value;
    const removed = await store.clearOld(days);
    ElMessage.success(`已清理 ${removed} 条日志`);
  } catch (e) {
    ElMessage.error(`清理失败：${e instanceof Error ? e.message : String(e)}`);
  } finally {
    // 无论成败都关闭对话框，避免卡在 loading
    clearVisible.value = false;
  }
}

// 挂载时拉取第一页；失败仅提示不阻塞页面
onMounted(() => {
  void store.fetchList().catch((e) => {
    ElMessage.error(`加载日志失败：${e instanceof Error ? e.message : String(e)}`);
  });
});
</script>

<style scoped>
/* 页面根容器：统一内边距 */
.page-logs {
  padding: 16px;
}

/* 顶部条：标题在左，清理入口在右 */
.page-header {
  align-items: center;
  display: flex;
  justify-content: space-between;
  margin-bottom: 12px;
}

/* 页面标题：与其他页面字号一致 */
.page-title {
  font-size: 20px;
  margin: 0;
}

/* 右侧清理操作组：横向小间距 */
.header-actions {
  align-items: center;
  display: flex;
  gap: 8px;
}

/* 清理范围下拉：固定较窄宽度 */
.clear-range {
  width: 130px;
}

/* 筛选条：横向 flex 自动换行，控件间留间距 */
.filter-bar {
  align-items: center;
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
  margin-bottom: 12px;
}

/* 多选筛选框统一宽度 */
.filter-select {
  width: 200px;
}

/* 关键字输入框宽度 */
.filter-keyword {
  width: 220px;
}

/* 错误提示条与表格留出间距 */
.error-bar {
  margin-bottom: 12px;
}

/* 表格占满宽度 */
.log-table {
  width: 100%;
}

/* 分页条：居中排列，按钮与页码留间距 */
.pager {
  align-items: center;
  display: flex;
  gap: 12px;
  justify-content: center;
  margin-top: 12px;
}

/* 当前页码文案：次要色 */
.pager-info {
  color: var(--el-text-color-secondary);
  font-size: 13px;
}

/* 详情区：纵向排列各信息行 */
.detail {
  display: flex;
  flex-direction: column;
  gap: 10px;
}

/* 概要行：类型/状态/时间横向排列 */
.detail-meta {
  align-items: center;
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
}

/* 概要里的时间/耗时文案：次要色小字 */
.detail-time {
  color: var(--el-text-color-secondary);
  font-size: 12px;
}

/* 普通信息行：标签在上，内容在下 */
.detail-row {
  display: flex;
  flex-direction: column;
  gap: 4px;
}

/* 字段标签：次要色小字 */
.detail-label {
  color: var(--el-text-color-secondary);
  font-size: 12px;
}

/* 字段值：常规字色 */
.detail-value {
  color: var(--el-text-color-regular);
  font-size: 13px;
}

/* 命令/输出/错误代码块：等宽字体 + 浅背景 + 可滚动 */
.detail-pre {
  background: var(--el-fill-color-light);
  border-radius: 4px;
  font-family: var(--el-font-family-mono, monospace);
  font-size: 12px;
  margin: 0;
  max-height: 200px;
  overflow: auto;
  padding: 8px 12px;
  white-space: pre-wrap;
  word-break: break-all;
}

/* 错误代码块：红色文字突出 */
.detail-error {
  color: var(--el-color-danger);
}

/* 中文错误提示：信息蓝背景，便于用户快速理解 */
.detail-hint {
  background: var(--el-color-info-light-9);
  border-radius: 4px;
  color: var(--el-text-color-regular);
  font-size: 13px;
  line-height: 1.6;
  margin: 0;
  padding: 8px 12px;
}
</style>
