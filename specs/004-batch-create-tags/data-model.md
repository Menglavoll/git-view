# 数据模型、错误处理和日志方案

## 持久化策略

本特性默认不新增数据库表和迁移：批量配置、预检查结果和执行状态只保存在当前 Vue 弹窗及后端 command 调用期间。应用退出后未完成任务不恢复。

操作结果通过现有 `operation_logs` 持久化，满足历史审计和失败诊断需求。

## 非持久化模型

建议在 `src-tauri/src/models/repository.rs` 或独立 `tag.rs` 中定义：

```rust
pub enum RemoteTagType {
    Lightweight,
    Annotated,
}

pub struct RemoteBranch {
    pub name: String,
    pub is_default: bool,
}

pub struct BranchHead {
    pub branch: String,
    pub sha: String,
    pub short_sha: String,
    pub subject: String,
}

pub struct CreateTagItem {
    pub repo_id: String,
    pub branch: String,
    pub tag_name: String,
    pub tag_type: RemoteTagType,
    pub message: Option<String>,
}

pub struct BatchCreateTagRequest {
    pub items: Vec<CreateTagItem>,
    pub concurrency: u32,
}
```

执行结果使用项目级条目，包含 `prechecked_sha`、`actual_sha` 和 `duration_ms`，支持判断分支在确认后是否发生变化。

## Tag ref 校验

在后端提供纯函数 `validate_tag_name(&str) -> Result<()>`，前端同步提供即时校验，但以后端为最终准入条件。

校验内容：

- 非空；
- 不是 `.` 或 `..`；
- 不含空格、控制字符或 `@{`；
- 不以 `/` 开头或结尾；
- 不包含连续 `..`；
- 不以 `.lock` 结尾；
- 不含不可作为 Git ref 的其他特殊形式。

不限制 `v1.2.0`、`release-2026-09`、`production` 等命名风格。

## 错误模型

在 `GitViewError` 中增加可被前端识别的变体，具体名称可按当前错误枚举风格调整：

```rust
TagNameInvalid(String),
TagAlreadyExists(String),
BranchNotFound(String),
TagPermissionDenied(String),
RateLimited(String),
AnnotatedTagUnsupported(String),
BatchValidation(String),
```

每个项目错误必须包含稳定错误码和脱敏 detail。前端错误文案按错误码本地化，平台原始响应只作为诊断 detail，不直接暴露 Token 或完整请求 URL。

## 预检查结果

预检查结果不入库，包含：

- 项目和账号；
- 分支；
- 分支 head；
- Tag 类型、名称和说明；
- Tag 是否已存在；
- `can_create`；
- 稳定错误码和用户提示。

同一项目重复出现在请求中属于批量校验失败。不同项目使用相同 Tag 名称合法。

## 操作日志

新增 `OperationType::CreateTag`，或新增 `BatchCreateTag` 与 `CreateTag` 两种类型。推荐使用两种类型：

- `BatchCreateTag`：批量操作汇总，target 为“批量创建 Tag（N 个项目）”；
- `CreateTag`：每个项目一条明细，target 为项目全名。

命令字段建议包含脱敏后的操作摘要，例如：

```text
create_tag repo=owner/project branch=main tag=v1.2.0 type=lightweight
```

附注说明可以记录，但建议截断到 2000 字符并保持脱敏策略；日志不记录 Token。

## 取消和重试

取消不需要持久化取消令牌。后端任务调度器在内存中维护待执行队列，取消时将等待项目标记为 `cancelled`，已运行项目继续完成。

重试不复用旧的 SHA 或旧的 Tag 存在性判断，必须重新读取和检查；重试结果作为新的批量操作日志记录。
