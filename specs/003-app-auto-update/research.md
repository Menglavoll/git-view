# Phase 0 研究：应用内自动更新技术决策

**Feature**: 003-app-auto-update | **Date**: 2026-07-13

本文件记录进入实现前的关键技术决策与取舍，供评审与后续维护追溯「为何这么做」。

## 决策 1：更新方案选型 —— `tauri-plugin-updater` vs 自建「检查+跳转下载」

**决策**：采用 Tauri 官方 `tauri-plugin-updater`（应用内下载安装）。

**背景**：用户在需求澄清中明确选择「应用内自动下载安装」，而非「检查到新版后跳转浏览器手动下载」。

**候选对比**：

| 维度 | 自建检查 + 跳转下载 | tauri-plugin-updater（选定） |
|---|---|---|
| 满足「自动下载安装」需求 | ❌ 仅能提示 + 跳浏览器 | ✅ 原地下载、验签、安装、重启 |
| 新增依赖 | 0 | 2 Rust + 2 前端官方插件 |
| 更新包签名要求 | 无 | minisign 强制验签 |
| 下载进度 / 跨平台安装 | 需自行实现 | 插件内置 |
| 与项目现状契合 | 高（复用 github_service HTTP 范式） | 中（引入新体系 + 发版基建） |

**理由**：官方 updater 是 Tauri 生态唯一成熟的原地更新方案，自带下载进度回调、minisign 验签、三平台安装逻辑，避免自行实现下载/替换二进制的高风险工作。代价是引入插件依赖与签名基建，已作为「有意例外」记入 plan.md 的 Complexity Tracking 并获用户确认。

## 决策 2：两种「签名」的辨析 —— updater 可在未做 OS 代码签名时工作

**决策**：本特性只做 **minisign 更新包签名**（updater 必需，免费），**不做** OS 代码签名/公证（付费证书，超出范围）。

**关键澄清**（纠正需求讨论初期的误判）：

- **OS 代码签名/公证**：Apple 开发者证书、Windows 代码签名证书。缺失只导致**首次运行/安装时系统弹「无法验证开发者」警告**（README 已有绕过说明）。它**不是** updater 的前置条件。
- **minisign 更新包签名**：`tauri signer generate` 免费生成密钥对，用于校验下载的更新包完整性、防篡改。这是 `tauri-plugin-updater` **强制**且**不可关闭**的安全机制。

**结论**：在未做 OS 代码签名的情况下，updater 的「下载→验签→安装→重启」链路**可以跑通**；用户仅在安装/首次运行时看到系统警告，体验与现状「手动下载 Release 包」一致，没有变差。

## 决策 3：更新清单 endpoint —— GitHub 稳定跳转，无需自建服务器

**决策**：endpoint 配置为
`https://github.com/menglavol/git-view/releases/latest/download/latest.json`

**理由**：
- GitHub 的 `releases/latest/download/<asset>` 是稳定跳转，始终指向最新正式 Release 的对应资产，无需在发版时更新 endpoint。
- `latest.json`（updater 标准清单）由 `tauri-action` 在发版时自动生成并作为 Release 资产上传，包含各平台包的下载 URL 与签名。
- 免去自建更新服务器的运维成本，与项目「轻量、无后端服务」定位一致。

**限流说明**：该路径为静态资产下载，不受 GitHub API 60 次/时匿名限流约束；手动点检查的频率不构成问题。

## 决策 4：版本比对 —— 交给 updater，客户端不自写

**决策**：不自写语义化版本比较；由 updater 用 `tauri.conf.json` 的 `version` 与 `latest.json` 的 `version` 自动比对。

**理由**：updater 内部已实现版本判定（发现 `latest.json` 版本高于当前应用即返回 `Update` 对象，否则返回 `null`）。自写比较属重复造轮子，且易在预发布后缀（`-beta`）等边界出错。当前应用版本从 `tauri.conf.json` 单一来源读取，不硬编码。

## 决策 5：更新逻辑落前端，后端仅注册插件

**决策**：不新增后端 `#[tauri::command]`；更新的检查/下载/安装由前端直接调用 `@tauri-apps/plugin-updater` 与 `@tauri-apps/plugin-process` 的 JS API，后端仅在 `lib.rs` 注册插件、在 capabilities 授权。

**理由**：
- Tauri 官方推荐前端直调 updater 插件 API，这是插件的设计用法。
- 项目既有取舍是「能力优先用插件，命令仅在需统一脱敏/聚合时才新增」。更新流程无敏感信息脱敏需求（不涉及 Token），无需经后端中转。
- 前端仍通过 `src/api/update.api.ts` 收拢插件调用，组件不直接 import 插件——保持「组件不直调底层」的既有分层。

## 待验证项（进入 Phase 1 / 实现时确认）

1. **macOS bundle 格式**：updater 需要 `.app.tar.gz` 而非 `.dmg`。需核对 `tauri.conf.json` 的 `bundle.targets: "all"` 是否产出 updater 兼容格式，不足则显式追加。
2. **Linux**：`.deb` 不支持原地更新，需确保 endpoint 指向 `.AppImage`；deb 安装用户走手动下载，UI 文案需说明。
3. **代理支持**：确认 `tauri-plugin-updater` 的 `Builder` / `check()` 是否暴露代理配置项。若不支持用户在「设置→网络」里配的代理，则文案注明「更新检查走系统网络/直连」。
4. **`process` 插件权限标识**：确认重启所需的确切 permission 标识（`process:allow-restart` 或 `process:default`），以最小授权为准。
