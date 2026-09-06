//! 操作日志服务。
//!
//! 提供两层能力：
//!   1. 轻量出口（US1 起）：`record_error` / `scrub`，强制 `redact_token` 脱敏，
//!      作为宪法 Principle III 的全局防御层（即便上游忘记脱敏也会被截获）。
//!   2. 持久化日志（US6）：`record_operation` 写入 `operation_logs` 表，
//!      配套 `list_operations` / `get_operation_detail` /
//!      `clear_operations_older_than` 查询与清理，以及 `translate_error`
//!      常见错误中文翻译。
//!
//! 安全约束：所有写库路径在落库前对全部文本字段执行 `redact_token`，
//! 数据库内绝不留存 token 明文（SC-009 / SC-010）。
//!
//! 设计沿用项目「无状态自由函数 + 显式传 `&DbPool`」模式，不引入单例：
//! 自由函数便于单元测试直接传入临时库，也避免全局状态带来的初始化顺序问题。

use rusqlite::OptionalExtension;

use crate::db::pool::DbPool;
use crate::errors::Result;
use crate::models::operation_log::{LogFilter, OperationLog, OperationStatus, OperationType};
use crate::utils::redact::redact_token;
use crate::utils::time::{now_iso8601, parse_iso8601};

/// 记录一次操作错误。
///
/// 当前实现：以 WARN 级别写入 tracing。消息会先经过 `redact_token` 脱敏。
///
/// # Arguments
///
/// * `operation` - 操作名（如 `add_account` / `test_connection` / `clone`）
/// * `message` - 原始错误消息（可能含 token，会被脱敏后再记录）
pub fn record_error(operation: &str, message: &str) {
    // 进 tracing 前先脱敏：日志文件同样属于「不得出现 token」的落地介质
    let safe = redact_token(message);
    tracing::warn!(operation = operation, "{}", safe);
}

/// 对错误消息执行一次脱敏并返回。
///
/// 供 service 层在向上传递错误前显式调用：`return Err(GitViewError::Network(scrub(&raw)))`。
/// 与 `record_operation` 内部脱敏互补：那是「落库」防线，这是错误「向上抛给用户」前的防线，
/// 两道叠加确保 token 既不进数据库、也不会出现在 UI 的错误提示里。
#[must_use]
pub fn scrub(message: &str) -> String {
    redact_token(message)
}

// =====================================================================
// US6：操作日志持久化（record / list / detail / clear）
// =====================================================================

/// 记录一次操作日志到 `operation_logs` 表。
///
/// 安全约束（宪法 Principle III，全局防御层）：写库前对 `target` / `command` /
/// `output` / `error_message` 全部执行 `redact_token`，即便上游已脱敏也再过一次。
///
/// 设计取舍：日志写入失败**不应阻断主操作**——本函数返回 `Result`，但调用方
/// 通常以 `let _ =` 忽略；失败时已在内部记 `tracing::warn`。
///
/// # Arguments
///
/// * `pool` - 数据库连接池
/// * `op_type` - 操作类型
/// * `target` - 操作目标简述（仓库名 / 账号名，将被脱敏）
/// * `status` - 结果状态
/// * `command` - 执行的命令（可空，将被脱敏）
/// * `output` - 命令输出摘要（可空，将被脱敏）
/// * `error_message` - 错误信息（失败时填充，可空，将被脱敏）
/// * `duration_ms` - 操作耗时（毫秒）
#[allow(clippy::too_many_arguments)]
pub fn record_operation(
    pool: &DbPool,
    op_type: OperationType,
    target: &str,
    status: OperationStatus,
    command: Option<&str>,
    output: Option<&str>,
    error_message: Option<&str>,
    duration_ms: u64,
) -> Result<()> {
    // 全字段脱敏，确保库内无 token 明文。
    // 这是「最后一道防线」：即使上游某处忘了脱敏，token 也会在写库前被拦下；
    // redact 幂等且无副作用，因此宁可对已脱敏内容重复执行一次也不省这步。
    let target = redact_token(target);
    let command = command.map(redact_token);
    let output = output.map(redact_token);
    let error_message = error_message.map(redact_token);

    // 主键用 UUID 而非自增：日志可能从多处并发写入，UUID 免去自增锁竞争
    let id = uuid::Uuid::new_v4().to_string();
    let occurred_at = now_iso8601();
    let op_str = op_type_to_str(op_type);
    let status_str = status_to_str(status);
    // SQLite INTEGER 是 i64；duration_ms 是 u64，理论上限大于 i64。
    // 实际操作耗时绝不会达到该量级，故溢出时兜底成 i64::MAX，而非报错中断写库。
    let duration = i64::try_from(duration_ms).unwrap_or(i64::MAX);

    // move 闭包把脱敏后的值移进连接作用域；占位符参数化 INSERT，杜绝 SQL 注入
    let result = pool.with_conn(move |conn| {
        conn.execute(
            "INSERT INTO operation_logs (
                id, operation_type, target, status,
                command, output, error_message, duration_ms, occurred_at
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            rusqlite::params![
                id,
                op_str,
                target,
                status_str,
                command,
                output,
                error_message,
                duration,
                occurred_at,
            ],
        )?;
        Ok(())
    });

    // 写库失败只记 warn、不上抛：日志是辅助审计，绝不能因为「记日志失败」
    // 而让用户真正在意的 push / commit 等主操作跟着失败。
    if let Err(ref e) = result {
        tracing::warn!("写入操作日志失败：{e}");
    }
    result
}

/// 按筛选条件查询操作日志（分页，按发生时间倒序）。
///
/// 查询结果会用 `translate_error` 为每条失败日志动态填充中文翻译
/// （不入库，翻译表升级后历史日志也受益）。
/// 倒序排列：日志面板默认让用户先看到最近发生的操作。
pub fn list_operations(pool: &DbPool, filter: &LogFilter) -> Result<Vec<OperationLog>> {
    // 动态拼接 WHERE 子句（全部参数化，避免 SQL 注入）。
    // 用 Box<dyn ToSql> 装异构参数：类型/状态是字符串、关键字是 String、
    // 分页是整数，装箱后才能塞进同一个 Vec 统一传给 query_map。
    let mut where_clauses: Vec<String> = Vec::new();
    let mut params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

    // 操作类型多选：用 IN (?, ?, ...) 动态生成等量占位符
    if !filter.operation_types.is_empty() {
        let placeholders = vec!["?"; filter.operation_types.len()].join(", ");
        where_clauses.push(format!("operation_type IN ({placeholders})"));
        for op in &filter.operation_types {
            params.push(Box::new(op_type_to_str(*op).to_string()));
        }
    }
    // 状态多选：同上，与类型条件用 AND 组合
    if !filter.statuses.is_empty() {
        let placeholders = vec!["?"; filter.statuses.len()].join(", ");
        where_clauses.push(format!("status IN ({placeholders})"));
        for s in &filter.statuses {
            params.push(Box::new(status_to_str(*s).to_string()));
        }
    }
    // 关键字仅匹配 target（仓库/账号名）：command/output 可能很长且已脱敏，
    // 对它们做 LIKE 既慢又容易命中无意义片段，故只在 target 上模糊匹配。
    if let Some(kw) = filter.keyword.as_ref().filter(|s| !s.is_empty()) {
        where_clauses.push("target LIKE '%' || ? || '%'".to_string());
        params.push(Box::new(kw.clone()));
    }

    // 无任何筛选条件时 WHERE 留空，等价全表查询（再交由下方分页截取）
    let where_sql = if where_clauses.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", where_clauses.join(" AND "))
    };

    // 分页：page 从 0 起，page_size 至少 1（防 0 导致 LIMIT 0 查不到数据）。
    // offset 用 saturating_mul：极端大的 page 不会溢出 panic，只会落到空页。
    let page_size = filter.page_size.max(1);
    let offset = filter.page.saturating_mul(page_size);
    params.push(Box::new(i64::from(page_size)));
    params.push(Box::new(i64::from(offset)));

    let sql = format!(
        "SELECT id, operation_type, target, status, command, output,
                error_message, duration_ms, occurred_at
         FROM operation_logs
         {where_sql}
         ORDER BY occurred_at DESC
         LIMIT ? OFFSET ?"
    );

    pool.with_conn(move |conn| {
        let mut stmt = conn.prepare(&sql)?;
        // Box<dyn ToSql> → &dyn ToSql 切片喂给 query_map。
        // 解两层引用 &**b：b 是 &Box<dyn ToSql>，先解 Box 再取内层 dyn 的引用。
        let param_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|b| &**b).collect();
        let rows = stmt.query_map(param_refs.as_slice(), row_to_log)?;
        let mut logs = Vec::new();
        for row in rows {
            logs.push(row?);
        }
        Ok(logs)
    })
}

/// 按 ID 查询单条操作日志详情（含动态错误翻译）。
///
/// 返回 `Option`：ID 不存在时返回 `None` 而非报错，交由前端提示「记录不存在」。
pub fn get_operation_detail(pool: &DbPool, id: &str) -> Result<Option<OperationLog>> {
    let id = id.to_string();
    pool.with_conn(move |conn| {
        // optional()：把「查无此行」从错误转成 Ok(None)，区别于真正的查询错误
        let log = conn
            .query_row(
                "SELECT id, operation_type, target, status, command, output,
                        error_message, duration_ms, occurred_at
                 FROM operation_logs WHERE id = ?1",
                rusqlite::params![id],
                row_to_log,
            )
            .optional()?;
        Ok(log)
    })
}

/// 清理操作日志。
///
/// * `before_days = None`：清空全部日志
/// * `before_days = Some(n)`：删除 n 天前的日志
///
/// 返回删除的行数。**属删除操作**：前端调用前须经 `ConfirmDangerDialog`
/// 二次确认（宪法 Principle III）。
pub fn clear_operations_older_than(pool: &DbPool, before_days: Option<u32>) -> Result<usize> {
    pool.with_conn(move |conn| {
        let affected = match before_days {
            // None：不带 WHERE，直接清空整表
            None => conn.execute("DELETE FROM operation_logs", [])?,
            Some(days) => {
                // 计算截止时间点：当前时间往前推 days 天。
                // 用 RFC3339 字符串比较：occurred_at 也以同格式存储，
                // ISO8601 文本按字典序即等价于时间序，故可直接用 `<` 比较。
                let cutoff =
                    (chrono::Utc::now() - chrono::Duration::days(i64::from(days))).to_rfc3339();
                conn.execute(
                    "DELETE FROM operation_logs WHERE occurred_at < ?1",
                    rusqlite::params![cutoff],
                )?
            }
        };
        Ok(affected)
    })
}

// =====================================================================
// T094：常见错误中文翻译表
// =====================================================================

/// 常见错误原始消息 → 中文翻译映射。
///
/// 使用 `contains` 子串匹配以容忍不同 Git / 平台版本的措辞差异。
/// 仅收录高频且用户可自助解决的错误；其余回退展示原文，避免翻译表无限膨胀。
static ERROR_TRANSLATIONS: &[(&str, &str)] = &[
    // 凭据无效：最常见的失败，引导用户检查 Token / SSH / 权限
    (
        "Authentication failed",
        "认证失败，请检查 Token、SSH Key 或账号权限。",
    ),
    // 仓库不可见：可能是路径错，也可能是私有库无权限
    (
        "Repository not found",
        "仓库不存在，或当前账号没有访问权限。",
    ),
    // SSH 公钥未被接受
    (
        "Permission denied (publickey)",
        "SSH Key 未配置或无权限，请检查本机 SSH 配置。",
    ),
    // DNS / 网络 / 代理问题
    (
        "Could not resolve host",
        "无法解析主机，请检查网络连接、DNS 或代理设置。",
    ),
    // clone 目标目录非空
    (
        "already exists and is not an empty directory",
        "目标目录已存在且不为空，请更换目录或选择跳过该仓库。",
    ),
    // 推送落后于远端，需先 pull
    (
        "non-fast-forward",
        "推送被拒绝（远程有新提交），请先执行 Pull。",
    ),
    // TLS 握手异常，多为网络中间设备或证书问题
    (
        "TLS connection was non-properly terminated",
        "TLS 连接中断，请检查网络与证书配置。",
    ),
];

/// 将常见错误原始消息翻译为中文友好提示。
///
/// 命中任一已知模式时返回对应中文；否则返回 `None`（前端回退展示原文）。
/// 按表序返回首个命中项，因此把更具体的模式排在更宽泛的之前可避免误命中。
#[must_use]
pub fn translate_error(raw: &str) -> Option<String> {
    // 子串匹配而非精确相等：容忍不同 Git / 平台版本对同一错误的措辞差异
    ERROR_TRANSLATIONS.iter().find_map(|(pattern, zh)| {
        if raw.contains(pattern) {
            Some((*zh).to_string())
        } else {
            None
        }
    })
}

// =====================================================================
// 内部：枚举 ↔ 字符串映射与行解析
// =====================================================================

/// 操作类型 → 入库字符串（snake_case，与 serde 序列化一致）。
///
/// 手写映射而非依赖 Display：入库字符串是持久化契约，必须与表里历史数据
/// 严格一致；集中在此显式定义，可避免枚举改名时悄悄改变库内取值。
const fn op_type_to_str(op: OperationType) -> &'static str {
    match op {
        OperationType::AddAccount => "add_account",
        OperationType::DeleteAccount => "delete_account",
        OperationType::TestConnection => "test_connection",
        OperationType::SyncRepos => "sync_repos",
        OperationType::Clone => "clone",
        OperationType::Fetch => "fetch",
        OperationType::Pull => "pull",
        OperationType::Push => "push",
        OperationType::Commit => "commit",
        OperationType::Checkout => "checkout",
        OperationType::CreateBranch => "create_branch",
        OperationType::BatchCreateTag => "batch_create_tag",
        OperationType::CreateTag => "create_tag",
        OperationType::ScanRepos => "scan_repos",
        OperationType::DiscardChanges => "discard_changes",
    }
}

/// 入库字符串 → 操作类型。库内值均由 `op_type_to_str` 写入，`_` 分支
/// 同时覆盖 `"discard_changes"` 与理论上不可达的未知值。
///
/// 与 `op_type_to_str` 严格互为逆映射，新增操作类型时两处必须同步修改。
fn op_type_from_str(s: &str) -> OperationType {
    match s {
        "add_account" => OperationType::AddAccount,
        "delete_account" => OperationType::DeleteAccount,
        "test_connection" => OperationType::TestConnection,
        "sync_repos" => OperationType::SyncRepos,
        "clone" => OperationType::Clone,
        "fetch" => OperationType::Fetch,
        "pull" => OperationType::Pull,
        "push" => OperationType::Push,
        "commit" => OperationType::Commit,
        "checkout" => OperationType::Checkout,
        "create_branch" => OperationType::CreateBranch,
        "batch_create_tag" => OperationType::BatchCreateTag,
        "create_tag" => OperationType::CreateTag,
        "scan_repos" => OperationType::ScanRepos,
        // 兜底到 DiscardChanges：它是最后一个分支，正常库内只会是该值或上列之一
        _ => OperationType::DiscardChanges,
    }
}

/// 操作状态 → 入库字符串。
///
/// 与 `op_type_to_str` 同理：入库字符串是持久化契约，集中显式定义。
const fn status_to_str(s: OperationStatus) -> &'static str {
    match s {
        OperationStatus::Success => "success",
        OperationStatus::Failed => "failed",
        OperationStatus::Cancelled => "cancelled",
    }
}

/// 入库字符串 → 操作状态（`_` 兜底为 Success）。
///
/// 兜底成 Success 而非报错：状态列由本模块写入、取值可控，未知值只会出现在
/// 人为改库等异常场景，此时按成功处理也不会误导用户把正常操作看成失败。
fn status_from_str(s: &str) -> OperationStatus {
    match s {
        "failed" => OperationStatus::Failed,
        "cancelled" => OperationStatus::Cancelled,
        _ => OperationStatus::Success,
    }
}

/// 把一行 `operation_logs` 记录映射为 `OperationLog`，并填充错误翻译。
///
/// 按列名（而非列序号）读取：即便日后调整 SELECT 列顺序也不会错位取值。
fn row_to_log(row: &rusqlite::Row<'_>) -> rusqlite::Result<OperationLog> {
    let op_str: String = row.get("operation_type")?;
    let status_str: String = row.get("status")?;
    let occurred_str: String = row.get("occurred_at")?;
    let error_message: Option<String> = row.get("error_message")?;
    // 动态填充中文翻译（不入库）：翻译只在读取时计算，这样以后扩充翻译表，
    // 早先写入的历史日志也能立刻享受新翻译，无需回填或迁移数据。
    let translated_error_message = error_message.as_deref().and_then(translate_error);

    Ok(OperationLog {
        id: row.get("id")?,
        operation_type: op_type_from_str(&op_str),
        target: row.get("target")?,
        status: status_from_str(&status_str),
        command: row.get("command")?,
        output: row.get("output")?,
        error_message,
        translated_error_message,
        // duration_ms 入库为非负 i64，转 u64 不丢符号（max(0) 兜底异常负值）
        #[allow(clippy::cast_sign_loss)]
        duration_ms: row.get::<_, i64>("duration_ms")?.max(0) as u64,
        // occurred_at 解析失败映射成 rusqlite 转换错误，让整行查询失败而非静默用错时间
        occurred_at: parse_iso8601(&occurred_str).map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(
                0,
                rusqlite::types::Type::Text,
                Box::new(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    e.to_string(),
                )),
            )
        })?,
    })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    /// 注入伪造 PAT，scrub 后不应保留原 token 字节
    #[test]
    fn scrub_strips_github_pat() {
        let raw = "fetch failed for ghp_abcdefghij1234567890ABCDEFGHIJ123456"; // allow-token-pattern: 测试样本
        let cleaned = scrub(raw);
        // 既要出现占位符，也要确认原始字节彻底消失（双向断言更难漏判）
        assert!(cleaned.contains("<REDACTED-TOKEN>"));
        assert!(!cleaned.contains("ghp_abcdefghij1234567890ABCDEFGHIJ123456")); // allow-token-pattern: 测试样本
    }

    /// 注入 GitLab PAT，scrub 后不应保留原 token 字节
    #[test]
    fn scrub_strips_gitlab_pat() {
        let raw = "401 with token glpat-abcdefghij1234567890"; // allow-token-pattern: 测试样本
        let cleaned = scrub(raw);
        // glpat- 前缀的 GitLab token 同样要被识别并替换
        assert!(cleaned.contains("<REDACTED-TOKEN>"));
        assert!(!cleaned.contains("glpat-abcdefghij1234567890")); // allow-token-pattern: 测试样本
    }

    /// 注入 URL 内联凭据，scrub 后不应保留密码部分
    #[test]
    fn scrub_strips_url_credentials() {
        let raw = "clone error: https://user:secret123@gitlab.example.com/x.git"; // allow-token-pattern: 测试样本
        let cleaned = scrub(raw);
        // 凭据被替换为 <REDACTED>@host，主机名保留以便用户定位是哪个远端
        assert!(cleaned.contains("<REDACTED>@gitlab.example.com"));
        assert!(!cleaned.contains("secret123"));
    }

    /// 干净文本应原样返回
    #[test]
    fn scrub_preserves_clean_message() {
        // 不含凭据的普通文本不应被改动，避免脱敏误伤正常日志
        let raw = "operation completed successfully";
        assert_eq!(scrub(raw), raw);
    }

    // -----------------------------------------------------------------
    // US6：持久化 + 翻译测试
    // -----------------------------------------------------------------

    use crate::db::migrations::run_pending_migrations;

    /// 建一个已完成迁移的临时数据库（返回的 `NamedTempFile` 须由调用方持有，
    /// 否则文件在函数返回后被删除）。
    ///
    /// 每个测试独享临时库，跑完即随 TempFile 析构删除，保证用例间零干扰、
    /// 可并行执行，也不会污染开发用的真实数据库。
    fn make_test_db() -> (DbPool, tempfile::NamedTempFile) {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let pool = DbPool::new(tmp.path()).unwrap();
        // 必须先迁移：record_operation 依赖 operation_logs 表结构已存在
        run_pending_migrations(&pool).unwrap();
        (pool, tmp)
    }

    /// 验收：record_operation 写入带 token 的消息后，读回应已脱敏且含中文翻译。
    ///
    /// US6 最关键的安全用例——在一次真实落库往返中同时验证「写库前脱敏」
    /// 与「读取时翻译」两条链路均生效。
    #[test]
    fn record_operation_redacts_token_and_translates() {
        let (pool, _tmp) = make_test_db();
        // 命令含 URL 内联 token，错误含独立 PAT：覆盖两种最常见的泄漏入口
        record_operation(
            &pool,
            OperationType::Push,
            "myrepo",
            OperationStatus::Failed,
            Some("git push https://ghp_abcdefghij1234567890ABCDEFGHIJ123456@github.com/x.git"), // allow-token-pattern: 测试样本
            None,
            Some("remote: Authentication failed for ghp_abcdefghij1234567890ABCDEFGHIJ123456"), // allow-token-pattern: 测试样本
            120,
        )
        .unwrap();

        let logs = list_operations(&pool, &LogFilter::default()).unwrap();
        assert_eq!(logs.len(), 1);
        let log = &logs[0];
        // command 与 error_message 均不得保留原始 token 字节
        assert!(!log.command.as_ref().unwrap().contains("ghp_")); // allow-token-pattern: 断言无 token
        assert!(!log.error_message.as_ref().unwrap().contains("ghp_")); // allow-token-pattern: 断言无 token
                                                                        // 失败消息含 "Authentication failed" → 应有中文翻译
        assert!(log.translated_error_message.is_some());
    }

    /// 验收：translate_error 命中常见错误，干净文本返回 None。
    ///
    /// 用例特意带上真实 Git 输出里的前后缀（`fatal:`、`ERROR:`、`(...)`），
    /// 验证 `contains` 子串匹配能容忍不同版本措辞，而非要求整串相等。
    #[test]
    fn translate_error_hits_common_cases() {
        // 四类高频失败都应命中翻译表
        assert!(translate_error("fatal: Authentication failed for 'https://x'").is_some());
        assert!(translate_error("ERROR: Repository not found.").is_some());
        assert!(translate_error("error: failed to push (non-fast-forward)").is_some());
        assert!(translate_error("Could not resolve host: github.com").is_some());
        // 不含任何已知模式的文本应返回 None，交前端回退展示原文
        assert!(translate_error("正常完成，无错误").is_none());
    }

    /// 验收：list_operations 按类型 / 状态 / 关键字筛选。
    ///
    /// 先写入 3 条覆盖不同类型/状态/target 的样本，再逐个维度断言筛选结果，
    /// 确认三类 WHERE 条件各自独立生效。
    #[test]
    fn list_operations_filters() {
        let (pool, _tmp) = make_test_db();
        // 样本 1：Commit / r1 / 成功
        record_operation(
            &pool,
            OperationType::Commit,
            "r1",
            OperationStatus::Success,
            None,
            None,
            None,
            10,
        )
        .unwrap();
        // 样本 2：Push / r1 / 失败（带错误信息）
        record_operation(
            &pool,
            OperationType::Push,
            "r1",
            OperationStatus::Failed,
            None,
            None,
            Some("boom"),
            20,
        )
        .unwrap();
        // 样本 3：Push / r2 / 成功
        record_operation(
            &pool,
            OperationType::Push,
            "r2",
            OperationStatus::Success,
            None,
            None,
            None,
            30,
        )
        .unwrap();

        // 只看 Push → 2 条（样本 2、3）
        let f = LogFilter {
            operation_types: vec![OperationType::Push],
            ..Default::default()
        };
        assert_eq!(list_operations(&pool, &f).unwrap().len(), 2);

        // 只看 Failed → 1 条且 target=r1（样本 2）
        let f = LogFilter {
            statuses: vec![OperationStatus::Failed],
            ..Default::default()
        };
        let logs = list_operations(&pool, &f).unwrap();
        assert_eq!(logs.len(), 1);
        assert_eq!(logs[0].target, "r1");

        // 关键字匹配 target → 1 条（样本 3 的 r2）
        let f = LogFilter {
            keyword: Some("r2".into()),
            ..Default::default()
        };
        assert_eq!(list_operations(&pool, &f).unwrap().len(), 1);
    }

    /// 验收：clear_operations_older_than(None) 清空全部。
    ///
    /// 写入 2 条后传 None 清空，断言返回删除行数为 2 且表变空，
    /// 验证「清空全部」分支（区别于按天数删除）正确执行。
    #[test]
    fn clear_operations_removes_all() {
        let (pool, _tmp) = make_test_db();
        // 写入 2 条不同 target 的日志作为待清理样本
        for name in ["r1", "r2"] {
            record_operation(
                &pool,
                OperationType::Commit,
                name,
                OperationStatus::Success,
                None,
                None,
                None,
                10,
            )
            .unwrap();
        }
        let removed = clear_operations_older_than(&pool, None).unwrap();
        // 返回值应等于实际删除行数，且清空后查询为空
        assert_eq!(removed, 2);
        assert_eq!(
            list_operations(&pool, &LogFilter::default()).unwrap().len(),
            0
        );
    }
}
