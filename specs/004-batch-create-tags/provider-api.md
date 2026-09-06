# Provider 和三平台 API 实施方案

## Provider 抽象

在 `src-tauri/src/services/provider.rs` 中新增统一能力：

```rust
async fn list_branches(&self, repo: &RemoteRepository) -> Result<Vec<RemoteBranch>>;

async fn get_branch_head(
    &self,
    repo: &RemoteRepository,
    branch: &str,
) -> Result<BranchHead>;

async fn check_tag(
    &self,
    repo: &RemoteRepository,
    tag_name: &str,
) -> Result<bool>;

async fn create_tag(
    &self,
    repo: &RemoteRepository,
    branch: &str,
    tag_name: &str,
    tag_type: RemoteTagType,
    message: Option<&str>,
) -> Result<CreatedRemoteTag>;
```

其中 `create_tag` 的输入是分支名，Provider 内部解析分支 head，避免前端直接提交不可信 SHA。返回值至少包含 Tag 名称、实际 commit SHA 和网页地址（如果平台返回）。

## 通用执行流程

```text
repo_id
  ↓ 读取本地仓库记录
account_id
  ↓ 读取账号配置和 keyring Token
Provider
  ↓ get_branch_head
commit SHA
  ↓ check_tag
Tag 是否已存在
  ↓ create_tag
平台 Tag
```

预检查和正式创建使用同一套 Provider 方法，但正式创建前必须重新执行 head 和 Tag 冲突检查。Tag 创建不是跨项目事务，任何项目失败都只影响自身。

## GitHub

### 分支 head

使用仓库分支接口或 commit ref 接口读取分支最新 commit SHA 和提交消息，映射为 `BranchHead`。

### 轻量 Tag

使用 Git References API 创建：

```text
POST /repos/{owner}/{repo}/git/refs
{
  "ref": "refs/tags/{tag_name}",
  "sha": "{branch_head_sha}"
}
```

创建前使用 Tag ref 查询判断是否已存在。

### 附注 Tag

使用 Git database 的两步流程：

1. 创建 Tag object，传入 tag 名称、message、object SHA、`commit` 类型；
2. 创建 `refs/tags/{tag_name}` 指向新建 Tag object。

创建者采用 GitHub API 当前 Token 对应身份；如果平台要求 tagger 字段，使用账号身份可获得的信息，不让用户手动伪造身份。两步流程第二步失败时不自动覆盖或重试创建已有对象，应将项目标记为失败并记录脱敏错误。

## GitLab

### 分支 head

使用：

```text
GET /projects/{id}/repository/branches/{branch}
```

读取 commit id 和 message。

### Tag 创建

轻量 Tag 和附注 Tag 优先使用 GitLab Repository Tags API。实现阶段需要根据当前 GitLab 版本确认附注 Tag 的说明字段和返回行为；如果该实例版本或 API 不支持附注 Tag，则返回 `annotated_tag_unsupported`，不得降级为轻量 Tag。

轻量创建的核心参数为：

```text
POST /projects/{id}/repository/tags
tag_name={tag_name}
ref={branch}
```

如果附注 Tag 需要额外字段，Provider 必须在预检查阶段能力探测或依据明确的 API 错误转换为“不支持附注 Tag”。

## Gitee

### 分支 head

使用仓库分支接口读取分支 commit SHA 和提交描述，复用现有 Gitee `auth_mode` 的 Header / Query Token 逻辑。

### Tag 创建

使用 Gitee 仓库 Tag 创建接口：

```text
POST /repos/{owner}/{repo}/tags
```

实现阶段以 Gitee 当前 Swagger 定义确认轻量和附注 Tag 的请求字段。若当前 API 仅支持轻量 Tag，附注 Tag 项目在预检查阶段返回 `annotated_tag_unsupported`，不能自动改为轻量 Tag。

## 权限与错误映射

Provider 必须沿用现有 Token 脱敏和平台错误映射。建议增加：

| 平台响应 | 统一错误 |
|---|---|
| 401 | `TokenInvalid` |
| 403 | `Forbidden` / `TagPermissionDenied` |
| 404 | `NotFound` / `BranchNotFound` |
| 409 / 已存在 | `TagAlreadyExists` |
| 422 / 参数非法 | `TagNameInvalid` 或 `Network` |
| 429 | `RateLimited` |
| 附注能力缺失 | `AnnotatedTagUnsupported` |

平台官方文档：GitHub Git References API、GitLab Tags API、Gitee API Swagger。
