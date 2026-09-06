//! GitView 业务服务层模块。
//!
//! 各 service 封装独立业务能力，被 commands 层调用：
//!   - `credential_service` — 凭据 keyring 读写
//!   - `provider`           — Git 托管平台抽象 trait
//!   - `github_service`     — GitHub Provider 实现
//!   - `gitlab_service`     — GitLab Provider 实现 + API URL 推导
//!   - `gitee_service`      — Gitee Provider 实现
//!   - `account_service`    — 账号 CRUD / 同步
//!   - `repository_service` — 远程仓库查询 / 收藏
//!   - `log_service`        — 操作日志（US6 完善）
//!   - `settings_service`   — 应用设置 key/value 读写（US7）
//!   - `proxy`              — 代理决策与应用（US7 / T105）

pub mod account_service;
pub mod clone_task_service;
pub mod credential_service;
pub mod data_dir_service;
pub mod git_cli_service;
pub mod git_reader_service;
pub mod gitee_service;
pub mod github_service;
pub mod gitlab_service;
pub mod log_maintenance;
pub mod log_service;
pub mod provider;
pub mod proxy;
pub mod repository_service;
pub mod settings_service;
pub mod tag_service;
