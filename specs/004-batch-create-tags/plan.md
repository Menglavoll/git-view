# Implementation Plan: 远程仓库批量创建 Tag

**Branch**: `004-batch-create-tags` | **Date**: 2026-09-06 | **Spec**: [spec.md](./spec.md)

## Summary

在现有远程仓库树形/列表选择和 Provider 架构上增加批量 Tag 工作流。前端负责项目配置、逐行编辑、预检查结果、摘要确认、进度和重试；后端负责账号与仓库真实性校验、平台 Provider 调用、并发调度、最终状态校验、部分失败隔离和日志。

本特性不新增持久化任务表，不恢复应用重启前的未完成任务；只通过现有操作日志保存批量摘要和项目明细。

## 技术约束

- 三平台仍通过 `GitHostingProvider` 统一抽象；前端不直接调用平台 API。
- 分支名由用户从远程分支列表筛选并选择，前端不接受未验证的任意分支输入。
- 预检查 SHA 仅用于用户核对；正式创建前重新读取分支 head，按执行时最新 SHA 创建。
- 不覆盖已有 Tag；不存在跨项目事务。
- 并发输入范围为 1～20，默认 3，项目总数不设上限。
- 附注 Tag 不被平台支持时预检查失败，不降级为轻量 Tag。

## 阶段门禁

1. 先完成三平台 API 能力核对，尤其是 GitLab/Gitee 附注 Tag 字段和权限。
2. 先完成 Provider 和后端 command 契约，再进行页面联调。
3. 先验证单项目轻量 Tag，再验证批量和部分失败。
4. 附注 Tag 在每个平台分别验证；不支持时必须验证阻止行为。
5. 所有写操作完成后再进行 UI、取消、重试和日志验收。

## 预计改动范围

```text
src/
├── api/remoteRepository.api.ts
├── components/repository/BatchCreateTagDialog.vue
├── components/repository/BatchCreateTagConfirmDialog.vue
├── pages/RemoteRepositories.vue
├── types/repository.ts 或 types/tag.ts
├── i18n/zh.ts、i18n/en.ts
└── api/error-messages.ts

src-tauri/src/
├── models/repository.rs 或 models/tag.rs
├── models/operation_log.rs
├── services/provider.rs
├── services/github_service.rs
├── services/gitlab_service.rs
├── services/gitee_service.rs
├── services/repository_service.rs
├── services/tag_service.rs
├── commands/remote_repositories.rs
├── services/log_service.rs
├── errors.rs
└── lib.rs
```

## 回退方案

- 某平台 Tag 写接口不通：保留该平台远程仓库和分支功能，批量 Tag 项目标记失败。
- 某平台不支持附注 Tag：只阻止该项目的附注 Tag，不影响轻量 Tag 和其他项目。
- 批量执行发生限流：按项目记录限流错误，用户可重试失败项目。
- 预检查和创建之间分支变化：按实际最新 SHA 创建并在结果中提示。
- 应用退出：不恢复内存任务，已提交远程请求的最终状态以平台实际结果为准。

## 完成定义

`tasks.md` 中 T001～T044 完成，`test-plan.md` 验收场景通过，三平台轻量 Tag 链路可用，附注 Tag 的支持或阻止结论有证据，质量门禁全部通过，且没有 Token 泄露或 Tag 覆盖行为。
