---
description: "Task list for feature 004-batch-create-tags"
---

# Tasks: 远程仓库批量创建 Tag

**Input**: `/specs/004-batch-create-tags/` 下的功能规格、交互设计、接口契约、Provider 方案、数据模型和测试计划。

**Prerequisites**: 先完成 `plan.md` 方案评审；未完成平台 API 能力确认前，不进入正式写接口实现。

## Phase 1: Setup and API research

- [ ] T001 记录当前前端、Rust、数据库迁移和操作日志质量门禁基线。
- [x] T002 [P] 核对 GitHub 轻量/附注 Tag API 请求和权限要求，并将 mock 响应固定到 Provider 测试。
- [x] T003 [P] 核对 GitLab 当前版本轻量/附注 Tag API 字段与权限要求；确认不支持时的错误映射。
- [ ] T004 [P] 核对 Gitee Swagger 当前 Tag 创建接口字段、轻量/附注能力和 Header/Query 鉴权行为。
- [ ] T005 根据 T002～T004 更新 `provider-api.md` 的平台能力结论；若平台能力与设计不符，先调整契约再编码。

## Phase 2: Foundational backend models and validation

- [x] T006 在 `src-tauri/src/models/` 增加 Tag、RemoteBranch、BranchHead、BatchTagRequest/Result 模型，并保持 camelCase 序列化。
- [x] T007 在 `src-tauri/src/services/provider.rs` 增加 RemoteBranch、BranchHead、Tag 类型和 Provider 方法抽象。
- [x] T008 在 `src-tauri/src/services/` 增加纯函数 Tag ref 校验和附注说明校验，补充边界单测。
- [x] T009 在 `src-tauri/src/errors.rs` 增加 Tag 已存在、分支不存在、Tag 名称非法、权限不足、限流、附注 Tag 不支持等错误码。
- [x] T010 在 `src-tauri/src/models/operation_log.rs` 增加 `BatchCreateTag` / `CreateTag` 操作类型，并更新日志序列化映射与前端类型。

## Phase 3: Provider implementation

- [x] T011 [P] 在 `github_service.rs` 实现分支 head、Tag 存在性检查、轻量 Tag 创建和附注 Tag 两步创建。
- [x] T012 [P] 在 `gitlab_service.rs` 实现分支 head、Tag 存在性检查和当前版本支持的 Tag 创建；不支持附注 Tag 时返回专用错误。
- [x] T013 [P] 在 `gitee_service.rs` 实现分支 head、Tag 存在性检查和 Tag 创建；保留 Header/Query 鉴权分支。
- [ ] T014 [P] 补充三平台 Provider mock HTTP 测试、分页/分支名编码测试和错误脱敏测试。
- [x] T015 统一三平台返回的 BranchHead 和 CreatedRemoteTag 字段，确保前端无需感知平台差异。

## Phase 4: Batch command and execution service

- [ ] T016 在 `repository_service.rs` 增加按 ID 批量读取并校验远程仓库的辅助，拒绝重复或不存在项目。
- [x] T017 在 `remote_repositories.rs` 增加 `get_remote_branch_head` command。
- [x] T018 在 `remote_repositories.rs` 增加 `precheck_batch_tags` command，逐项目返回失败原因和预检查 SHA。
- [x] T019 新增批量 Tag 执行 service：校验并发 1～20，建立有限并发队列，隔离项目失败，支持取消等待项。
- [x] T020 在执行前重新读取分支 head 和 Tag 状态，返回预检查 SHA 与实际 SHA，禁止覆盖已有 Tag。
- [x] T021 在执行 service 中记录批量摘要和项目明细日志，保证错误及命令字段脱敏。
- [x] T022 在 `lib.rs` 注册新增 commands，并确认现有 `list_remote_branches` 兼容策略。
- [ ] T023 补充批量执行 service 单测：成功、部分失败、取消、并发 20、SHA 变化、重复 Tag 和重试输入。

## Phase 5: Frontend API, types and dialog

- [x] T024 [P] 在 `src/types/repository.ts` 或新增 `src/types/tag.ts` 增加批量 Tag 类型。
- [x] T025 [P] 在 `src/api/remoteRepository.api.ts` 增加分支 head、预检查和批量创建 API 封装。
- [x] T026 新增 `BatchCreateTagDialog.vue`：项目表、添加/移除项目、统一 Tag 初始化、并发输入和逐行编辑。
- [x] T027 在批量页面实现分支惰性加载、关键字筛选、分支 head 展示和加载错误状态。
- [x] T028 实现轻量/附注 Tag 切换、逐项目说明输入、1～2000 字符校验和 Tag ref 即时校验。
- [x] T029 新增预检查结果状态和失败项目修正/移除流程，失败项目存在时禁用确认。
- [x] T030 新增 `BatchCreateTagConfirmDialog.vue`，展示项目、平台、账号、分支、预检查 SHA、Tag 类型、名称和说明。
- [x] T031 在 `RemoteRepositories.vue` 接入“批量添加 Tag”按钮、入口状态、选择清理和弹窗生命周期。
- [x] T032 更新 `RemoteRepoTree.vue` / `RemoteRepoTable.vue` 的事件或 props，确保目录选择和列表选择能复用批量 Tag 入口。

## Phase 6: Execution result, cancellation and retry

- [x] T033 实现执行进度、成功/失败/取消状态和剩余任务取消。
- [x] T034 实现结果列表，展示预检查 SHA、实际 SHA、SHA 变化提示、耗时和脱敏错误。
- [x] T035 实现“一键重试失败项目”，重试时重新获取分支 head、重新预检查并再次确认。
- [ ] T036 更新 `src/i18n/zh.ts`、`src/i18n/en.ts` 和 `src/api/error-messages.ts` 的文案与错误码。
- [x] T037 更新操作日志页面的操作类型筛选和详情展示，支持批量摘要与项目明细。

## Phase 7: Verification and delivery

- [ ] T038 [P] 补充前端纯函数/组件逻辑测试或可重复的测试脚本。
- [x] T039 [P] 更新 `docs/user-guide.md`，说明目录选择、预检查、Tag 类型、并发、失败重试和不覆盖规则。
- [x] T040 运行前端 lint、格式检查、类型检查和构建。
- [x] T041 运行 Rust fmt、clippy、单元测试和集成测试。
- [ ] T042 执行 `test-plan.md` 的三平台手动验收并记录证据。
- [ ] T043 复核中文注释比例、日志脱敏、无 debug 输出和无新增无关依赖。
- [ ] T044 完成功能规格、接口契约、平台 API 结论和验收记录的最终回填。

## 依赖与并行建议

- T002～T004 可并行，但 T005 依赖三者完成。
- T011～T014 依赖 T007 和 T005，三平台实现可并行。
- T024、T025 可并行；T026 依赖 T024/T025 和交互设计。
- 后端 Phase 3～4 与前端 Phase 5 可部分并行，但前端联调依赖 T017～T022。
- T033～T037 依赖预检查和创建 command 完成。
- T040～T043 依赖所有实现任务完成。

## 交付顺序

1. Provider API 核对和基础模型；
2. 分支 head 与预检查 command；
3. 单项目轻量 Tag 创建链路；
4. 批量执行、并发、取消和日志；
5. 前端配置、确认和结果页面；
6. 附注 Tag 三平台能力接入或按平台能力阻止；
7. 重试、文案、文档和全量验收。
