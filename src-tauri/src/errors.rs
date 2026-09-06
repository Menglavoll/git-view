//! GitView 后端统一错误类型与 Result 别名。
//!
//! 本模块是后端所有 service / command 的错误统一出口，提供：
//!   - `GitViewError` 枚举：覆盖 V1 MVP 范围内所有可被前端理解的错误类别
//!   - `Result<T>` 类型别名：所有 service / command 函数返回值的默认类型
//!   - 从常见底层错误类型（rusqlite / reqwest / keyring / std::io）的 `From` 转换
//!
//! 序列化约定：
//!   `#[serde(tag = "code", content = "detail")]` 将错误序列化为
//!   `{ "code": "TokenInvalid", "detail": "..." }` 形式；
//!   前端按 `code` 字段映射本地化文案（详见 `src/api/error-messages.ts`）。
//!
//! 安全要求（宪法 Principle III）：
//!   - 任何错误消息 MUST 在写入日志前经过 `utils::redact::redact_token` 脱敏
//!   - 错误的 `Display` 输出不得包含 Token、密码等敏感凭据明文
//!   - 当底层错误（如 reqwest）可能携带敏感信息时，应只取必要的简短描述

use std::io;

/// GitView 后端统一错误类型。
///
/// 序列化后可直接通过 Tauri IPC 返回给前端；前端按 `code` 字段映射中文文案。
///
/// 变体设计原则：
///   - 用户可理解：每个变体对应一个用户视角能区分的错误场景
///   - 可追溯：保留必要的 detail（已脱敏）便于日志诊断
///   - 不暴露敏感信息：底层 token / 密码必须在转换时被剥离
#[derive(Debug, thiserror::Error, serde::Serialize, Clone)]
#[serde(tag = "code", content = "detail")]
pub enum GitViewError {
    /// 数据库错误（SQLite 读写、迁移、Schema 校验等）。
    #[error("数据库错误：{0}")]
    Database(String),

    /// 凭据存储错误（系统密钥库不可用、读写失败、平台后端异常）。
    #[error("凭据存储错误：{0}")]
    Credential(String),

    /// Token 无效或已过期（HTTP 401 / GitLab 403 Token Revoked 等场景）。
    #[error("Token 无效或已过期")]
    TokenInvalid,

    /// API 地址格式错误或不可达（URL 解析失败、host 解析失败的提前判定）。
    #[error("API 地址错误：{0}")]
    ApiUrlInvalid(String),

    /// 网络错误（DNS 解析失败、连接超时、TCP 断连、TLS 握手失败等）。
    #[error("网络错误：{0}")]
    Network(String),

    /// TLS 证书错误（自签名证书且未启用白名单、CA 验证失败等）。
    #[error("TLS 证书错误：{0}")]
    TlsCert(String),

    /// 响应解析失败（HTTP 请求成功但响应体不符合预期结构，或返回了非 JSON 内容）。
    ///
    /// 与 `Network` 严格区分：连接已建立、状态码也正常，问题出在响应**内容**上。
    /// 常见诱因是 API 地址配错（打到了网页而非 API）、实例版本字段差异，或被
    /// 反向代理 / SSO 拦截返回登录页。对这类错误提示"检查网络或代理"会误导用户，
    /// 故单列一类以便前端给出"检查 API 地址 / 实例兼容性"的针对性引导。
    #[error("响应解析失败：{0}")]
    ResponseDecode(String),

    /// 权限不足（HTTP 403 且非 Token 失效场景，如仓库私有未授权访问）。
    #[error("权限不足")]
    Forbidden,

    /// 资源不存在（HTTP 404、本地路径不存在、数据库记录缺失等）。
    #[error("资源不存在：{0}")]
    NotFound(String),

    /// Git 命令执行失败（exit code != 0 或解析失败）。
    #[error("Git 命令执行失败：{0}")]
    GitCommand(String),

    /// Git 可执行文件未找到（PATH 中无 git、自定义路径无效）。
    #[error("Git 未安装或路径无效")]
    GitNotFound,

    /// 路径冲突（目标目录已存在且非空、目录已被其他仓库占用等）。
    #[error("路径冲突：{0}")]
    PathConflict(String),

    /// 远程仓库名称已被占用（创建仓库时平台返回「已存在」：GitHub 422 / GitLab、Gitee 400）。
    ///
    /// 单列一类而非复用 PathConflict：前端据此专门提示「改用其他仓库名」，
    /// 与本地路径冲突（建议换目录）的处理路径完全不同。
    #[error("远程仓库名称已存在")]
    RepoNameTaken,

    /// 路径不存在（用户输入的目录不可达、本地仓库被外部删除等）。
    #[error("路径不存在：{0}")]
    PathMissing(String),

    /// 用户取消操作（前端 ConfirmDangerDialog 拒绝、后端 cancel token 触发等）。
    #[error("用户取消")]
    UserCancelled,

    /// 账号同步互斥冲突（同一账号已有同步任务在执行中）。
    /// 对应 plan.md §Risks "多账号同时触发同步" 与 T030 的账号粒度互斥锁。
    #[error("账号 {0} 正在同步中，请稍后再试")]
    BusyAccount(String),

    /// 工作区脏检测失败（切换分支时存在未提交变更）。
    /// 对应 spec FR-044 与 T079/T086 的脏工作区阻断逻辑。
    #[error("工作区存在未提交变更，请先提交或暂存后再执行")]
    DirtyWorkdir,

    /// Tag 名称不符合 Git ref 规则，必须由用户修正后再创建。
    #[error("Tag 名称不合法：{0}")]
    TagNameInvalid(String),

    /// 目标远程仓库已存在同名 Tag；本应用不提供覆盖写入。
    #[error("Tag 已存在：{0}")]
    TagAlreadyExists(String),

    /// 用户选择的远程分支不存在或已被删除。
    #[error("远程分支不存在：{0}")]
    BranchNotFound(String),

    /// 当前平台或实例不能创建附注 Tag，不能静默降级为轻量 Tag。
    #[error("该平台不支持附注 Tag：{0}")]
    AnnotatedTagUnsupported(String),

    /// 平台返回限流，用户可以在稍后重试失败项目。
    #[error("平台 API 请求过于频繁，请稍后重试：{0}")]
    RateLimited(String),

    /// 内部错误（不应出现在生产路径上的兜底类型，定位 Bug 用）。
    #[error("内部错误：{0}")]
    Internal(String),
}

/// 后端统一 Result 别名。
///
/// service 与 command 层均使用本别名作为返回值类型，
/// 简化签名并统一错误传播路径。
pub type Result<T> = std::result::Result<T, GitViewError>;

// =====================================================================
// From 转换实现 —— 让底层错误能用 `?` 自动转换为 GitViewError
// =====================================================================
//
// 实现原则：
//   1. 转换时只保留必要的描述信息，丢弃可能含敏感数据的内部状态
//   2. reqwest 的错误优先按状态码/分类映射到具体变体，不简单包成 Network
//   3. 任何 token 字符串绝不放入 String 字段（调用方负责脱敏后再传入）

impl From<rusqlite::Error> for GitViewError {
    /// SQLite 错误映射：区分"记录不存在"与其他类型。
    fn from(value: rusqlite::Error) -> Self {
        // 对应 SELECT 等查询不命中行的常见路径
        if matches!(value, rusqlite::Error::QueryReturnedNoRows) {
            Self::NotFound("数据库记录不存在".to_string())
        } else {
            Self::Database(value.to_string())
        }
    }
}

impl From<keyring::Error> for GitViewError {
    /// keyring 错误映射：识别"凭据不存在"以便前端按"凭据缺失"展示。
    fn from(value: keyring::Error) -> Self {
        // 平台无关地识别 NoEntry（macOS Keychain、Windows Credential Manager、
        // Linux Secret Service 均会映射到此 variant）
        if matches!(value, keyring::Error::NoEntry) {
            Self::NotFound("凭据未存储".to_string())
        } else {
            Self::Credential(value.to_string())
        }
    }
}

impl From<reqwest::Error> for GitViewError {
    /// reqwest 错误映射：按 HTTP 状态码与错误分类精细化转换。
    ///
    /// 注意：reqwest::Error 的 Display 输出**可能**包含 URL，故在转换前
    /// 必须由 service 层保证 URL 中不含凭据片段（参考 utils::redact）。
    fn from(value: reqwest::Error) -> Self {
        // 优先按 HTTP 状态码分类
        if let Some(status) = value.status() {
            return match status.as_u16() {
                401 => Self::TokenInvalid,
                403 => Self::Forbidden,
                404 => Self::NotFound("远程资源不存在".to_string()),
                // 其他 4xx / 5xx 统一进入 Network 通道，由 service 自行细化
                _ => Self::Network(format!("HTTP 状态码 {status}")),
            };
        }

        // 无 HTTP 状态时按错误分类判断
        if value.is_timeout() {
            Self::Network("请求超时".to_string())
        } else if value.is_connect() {
            Self::Network("连接失败，请检查网络或代理设置".to_string())
        } else if value.is_decode() {
            // 响应体解析失败：归入 ResponseDecode 而非 Network，避免误导用户排查网络
            Self::ResponseDecode("响应内容无法解析".to_string())
        } else {
            // 兜底：使用 reqwest 自身的 Display 输出，但去掉可能的 URL 引用
            Self::Network(strip_url_from_message(&value.to_string()))
        }
    }
}

impl From<io::Error> for GitViewError {
    /// I/O 错误映射：区分 NotFound / PermissionDenied / 其他。
    fn from(value: io::Error) -> Self {
        match value.kind() {
            io::ErrorKind::NotFound => Self::PathMissing(value.to_string()),
            io::ErrorKind::PermissionDenied => Self::Forbidden,
            io::ErrorKind::AlreadyExists => Self::PathConflict(value.to_string()),
            _ => Self::Internal(value.to_string()),
        }
    }
}

impl From<url::ParseError> for GitViewError {
    /// URL 解析错误统一映射到 ApiUrlInvalid。
    fn from(value: url::ParseError) -> Self {
        Self::ApiUrlInvalid(value.to_string())
    }
}

impl From<serde_json::Error> for GitViewError {
    /// JSON 序列化/反序列化错误统一映射到内部错误。
    fn from(value: serde_json::Error) -> Self {
        Self::Internal(format!("JSON 处理错误：{value}"))
    }
}

/// 移除错误消息中可能出现的 URL 片段，避免日志中残留含凭据的 URL。
///
/// reqwest 在某些错误路径会把请求 URL 拼到 Display 输出里，本函数
/// 以"url:"为切分点截断后续内容；若无 URL 引用则原样返回。
fn strip_url_from_message(msg: &str) -> String {
    // 常见 reqwest 输出形如 "error sending request for url (https://...)"
    // 截断"for url"之后的内容，避免泄露含 token 的 URL
    msg.split_once(" for url")
        .map_or_else(|| msg.to_string(), |(prefix, _)| prefix.to_string())
}

// =====================================================================
// 单元测试 —— 覆盖关键转换路径与序列化形态
// =====================================================================
#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    /// 验证 Display 文案为中文且不含敏感占位（基本健壮性）
    #[test]
    fn display_messages_are_in_chinese() {
        let err = GitViewError::TokenInvalid;
        assert_eq!(err.to_string(), "Token 无效或已过期");

        let err = GitViewError::Database("test".to_string());
        assert!(err.to_string().contains("数据库错误"));
    }

    /// 验证 JSON 序列化格式符合 `{ code, detail }` 约定
    #[test]
    fn serialize_uses_tagged_code_detail() {
        let err = GitViewError::TokenInvalid;
        let json = serde_json::to_string(&err).unwrap();
        assert!(json.contains("\"code\":\"TokenInvalid\""));

        let err = GitViewError::Database("conn lost".to_string());
        let json = serde_json::to_string(&err).unwrap();
        assert!(json.contains("\"code\":\"Database\""));
        assert!(json.contains("conn lost"));

        // ResponseDecode 必须稳定序列化为该 code，前端 error-messages.ts 依赖此字面量映射文案
        let err = GitViewError::ResponseDecode("bad json".to_string());
        let json = serde_json::to_string(&err).unwrap();
        assert!(json.contains("\"code\":\"ResponseDecode\""));
        assert!(json.contains("bad json"));

        // RepoNameTaken 无 detail，但 code 必须稳定：前端据此映射「改名」提示
        let err = GitViewError::RepoNameTaken;
        let json = serde_json::to_string(&err).unwrap();
        assert!(json.contains("\"code\":\"RepoNameTaken\""));
    }

    /// 验证 rusqlite::Error::QueryReturnedNoRows → NotFound
    #[test]
    fn sqlite_no_rows_maps_to_not_found() {
        let err: GitViewError = rusqlite::Error::QueryReturnedNoRows.into();
        assert!(matches!(err, GitViewError::NotFound(_)));
    }

    /// 验证 keyring::Error::NoEntry → NotFound
    #[test]
    fn keyring_no_entry_maps_to_not_found() {
        let err: GitViewError = keyring::Error::NoEntry.into();
        assert!(matches!(err, GitViewError::NotFound(_)));
    }

    /// 验证 io::ErrorKind::NotFound → PathMissing
    #[test]
    fn io_not_found_maps_to_path_missing() {
        let err: GitViewError = io::Error::new(io::ErrorKind::NotFound, "missing").into();
        assert!(matches!(err, GitViewError::PathMissing(_)));
    }

    /// 验证 io::ErrorKind::AlreadyExists → PathConflict
    #[test]
    fn io_already_exists_maps_to_path_conflict() {
        let err: GitViewError = io::Error::new(io::ErrorKind::AlreadyExists, "exists").into();
        assert!(matches!(err, GitViewError::PathConflict(_)));
    }

    /// 验证 URL 解析错误 → ApiUrlInvalid
    #[test]
    fn url_parse_error_maps_to_api_url_invalid() {
        let err: GitViewError = url::Url::parse("not a url").unwrap_err().into();
        assert!(matches!(err, GitViewError::ApiUrlInvalid(_)));
    }

    /// 验证 strip_url_from_message 能去掉 reqwest 的 URL 尾巴
    #[test]
    fn strip_url_removes_for_url_suffix() {
        let s = "error sending request for url (https://api.github.com/user)";
        assert_eq!(strip_url_from_message(s), "error sending request");

        let s = "plain message without url";
        assert_eq!(strip_url_from_message(s), "plain message without url");
    }
}
