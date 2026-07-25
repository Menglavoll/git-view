# Implementation Plan: 应用内自动检查与更新

**Branch**: `003-app-auto-update` | **Date**: 2026-07-13 | **Spec**: [spec.md](./spec.md)

**Input**: 用户需求「在设置中添加检查软件更新功能，更新来源为本项目 GitHub」+ 已确认「采用应用内自动下载安装」方案

## Summary

在设置中心「通用」Tab 底部新增「关于与更新」分区，接入 Tauri 官方 `tauri-plugin-updater` 实现**应用内检查 → 下载 → 验签 → 安装 → 重启**的自动更新闭环。更新的权威来源为本项目 GitHub Releases（`menglavol/git-view`），CI 发版时由 `tauri-action` 自动为各平台安装包生成 minisign 签名并产出 `latest.json` 更新清单；客户端从 `releases/latest/download/latest.json` 拉取清单、用内置公钥验签后安装。

本特性由两部分组成，需按顺序落地：

1. **发布侧基建（P0，前置且一次性）**：生成 updater 签名密钥对、私钥入 GitHub Secrets、公钥入 `tauri.conf.json`、改造 `release.yml` 注入签名环境变量并核对各平台 updater 兼容 bundle 格式。**没有这一步，任何客户端更新代码都无法验签通过。**
2. **客户端更新能力（P1）**：注册 `updater` + `process` 插件、扩展 capabilities 权限、前端封装检查/下载/安装 API、`Settings.vue` 通用 Tab 新增「关于与更新」子组件（版本展示、检查按钮、发布说明、下载进度、重启引导）。

## Technical Context

**Language/Version**: Rust 1.75+（Tauri 2 后端）、TypeScript 5 + Vue 3.4（前端）

**Primary Dependencies（本特性新增，属对既有约束的有意例外，见 Constitution Check）**:
- Rust：`tauri-plugin-updater = "2"`、`tauri-plugin-process = "2"`
- 前端：`@tauri-apps/plugin-updater`、`@tauri-apps/plugin-process`

**Storage**: 不涉及 SQLite / keyring 变更。签名私钥存 GitHub Actions Secrets（不入库）；公钥明文写入 `tauri.conf.json`（公钥公开无风险）。

**Testing**: `cargo test`（版本比较若下沉后端则加纯函数单测；本方案主逻辑在前端插件侧，后端几乎无新逻辑）、前端手动 GUI 实测（沿用 [[feedback_frontend_workflow]]）、**一次真实发版验证**（发一个带签名的 Release 作基线，用旧版本客户端验证能检测并安装）。

**Target Platform**: macOS / Windows / Ubuntu（三平台更新兼容格式差异见「实施要点与风险」）

**Project Type**: 桌面应用（Tauri，单仓库 `src/` 前端 + `src-tauri/` 后端）

**Performance Goals**: 检查更新为手动触发的单次 HTTP 请求，秒级返回；下载显示实时进度，不阻塞 UI。

**Constraints**:
- 保留既有中文友好错误提示与失败不阻断策略（检查失败仅提示，不影响设置页其余功能）。
- **未做 OS 代码签名/公证**：更新安装后 macOS/Windows 仍会触发系统安全警告，沿用 README 既有绕过说明（体验不劣于现状的「手动下载 Release」）。
- 私钥一旦丢失，存量用户将无法再验签任何后续更新——私钥必须安全备份。
- `Settings.vue` 当前约 1100 行，更新逻辑抽为独立子组件 `AboutUpdate.vue`，避免触碰单文件 ≤500 行门禁。

**Scale/Scope**: 改动约 3 个后端文件（Cargo/lib/capabilities）+ 2 个前端新增文件（api + 组件）+ 3 个前端修改（Settings/zh/en）+ 2 个发布基建文件（tauri.conf/release.yml）+ 1 份发版操作文档。

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

### Gate I — 代码质量优先

- [x] 格式化：Rust `cargo fmt`；前端 `prettier`。
- [x] 静态分析：Rust `cargo clippy -- -D warnings`；前端 `eslint --max-warnings 0`。
- [x] 长度：更新交互抽到新组件 `AboutUpdate.vue`（预计 <200 行）；`Settings.vue` 仅新增 `<el-divider>` + 组件引用（约 +10 行），不突破 500 行。
- [x] 无临时调试输出 / 废弃注释 / 无关联 TODO。

### Gate II — 中文注释规范

- [x] 承诺所有新增/修改源文件中文注释比例 ≥ 0.3（[[feedback_comment_ratio_gate]]，门禁阈值 0.3；不足补注释而非豁免）。
- [x] 新增公共封装（`update.api.ts` 各方法、`AboutUpdate.vue` 交互）均加中文文档注释；「为何用 updater 而非自建检查」「未签名仍可更新」等非显然决策加 WHY 注释。
- [x] 无自动生成代码。

### Gate III — 文件操作安全 — **NON-NEGOTIABLE**

- [x] 识别删除点：本特性**不新增**破坏性文件操作。更新包的下载/替换由 `tauri-plugin-updater` 在其受控目录内完成，不涉及应用自身对用户文件的删除。
- [x] 安装即替换应用自身二进制属插件既定行为，安装前经 minisign 验签防篡改，安装后经 `process.relaunch()` 重启——均为插件官方受控流程。
- [x] 读/写操作（新增源码、追加配置）可直接执行。

### Gate IV — 方案确认优先 — **NON-NEGOTIABLE**

- [x] 本 plan.md 即方案，呈现给用户等待显式批准后再进入实施。
- [x] 方案含目标、关键步骤、文件清单、风险、回退。
- [x] **已就两点例外取得用户确认**：① 新增 2 个官方插件（违反 002-plan 的「不新增依赖」惯例，本特性有意例外）；② 未做 OS 代码签名，更新后仍有系统警告。
- [x] 实施中如遇平台 bundle 格式不兼容 updater、或发版签名链路异常，将暂停并二次确认。

### Gate 综合检查

- [x] CI 已有 fmt/clippy/eslint/注释比例门禁，本特性沿用；`release.yml` 改造后需验证发版流程仍绿。
- [x] 跨平台：三平台 updater 兼容格式差异已在风险表列明并逐一处置。

**结论**：本特性对「不新增第三方依赖」这一 002 惯例构成**有意例外**（功能必需，已获用户确认），记入 Complexity Tracking。其余无违规。

## Project Structure

### Documentation (this feature)

```text
specs/003-app-auto-update/
├── plan.md              # 本文件
├── spec.md              # 需求与验收标准
├── research.md          # Phase 0：技术决策（updater vs 自建、签名、endpoint）
├── data-model.md        # Phase 1：更新清单/结果数据结构
├── contracts/
│   └── updater-flow.md  # Phase 1：插件调用契约与前端 API 契约
├── quickstart.md        # Phase 1：发版基建操作步骤 + 验收自测
└── checklists/
    └── requirements.md  # 需求质量核对
```

### Source Code (repository root)

改动落点（现有目录，不新增顶层结构）：

```text
发布基建（一次性，部分需用户在 GitHub 后台操作）
├── src-tauri/tauri.conf.json      # plugins.updater：endpoints + pubkey
├── .github/workflows/release.yml  # 注入 TAURI_SIGNING_* 环境变量；核对 bundle 兼容格式
└── (GitHub Secrets)               # TAURI_SIGNING_PRIVATE_KEY / _PASSWORD（用户配置）

src-tauri/
├── Cargo.toml                     # +tauri-plugin-updater +tauri-plugin-process
├── src/lib.rs                     # 注册 updater + process 两个插件
└── capabilities/default.json      # +updater:default +process:allow-restart

src/
├── api/
│   └── update.api.ts              # [新] 封装 check() / downloadAndInstall(onProgress) / relaunch()
├── components/
│   └── settings/
│       └── AboutUpdate.vue        # [新] 关于与更新子组件（版本/检查/进度/重启）
├── pages/
│   └── Settings.vue               # 通用 Tab 底部新增「关于与更新」分区，引入子组件
└── i18n/
    ├── zh.ts                      # settings.update.* / settings.about.* 文案
    └── en.ts                      # 同步英文文案
```

**Structure Decision**: 沿用既有分层。更新逻辑主要落在前端（Tauri 官方推荐前端直调 updater 插件 API），后端仅做插件注册与权限授予，不新增 command——与项目「能力优先用插件、命令仅在需统一脱敏/聚合时新增」的取舍一致。

## Phase 0 摘要（详见 research.md）

- **为何用 `tauri-plugin-updater` 而非自建「检查+跳转」**：用户已明确要「应用内自动下载安装」，官方 updater 是唯一成熟的原地更新方案，自带下载进度、验签、跨平台安装。
- **两种签名辨析**：updater 要求的是 **minisign 更新包签名**（`tauri signer generate` 免费生成），与 **OS 代码签名/公证**（付费证书）是两回事。缺后者只影响首次运行系统警告，**不阻断** updater 工作。
- **endpoint 选择**：用 GitHub 稳定跳转 `https://github.com/menglavol/git-view/releases/latest/download/latest.json`，无需自建服务器；`latest.json` 由 `tauri-action` 在发版时自动生成上传。
- **版本判定**：由 updater 自身用 `tauri.conf.json` 的 `version` 与 `latest.json` 的 `version` 比对，客户端无需自写语义化比较。
- **代理**：updater 请求走系统网络栈；若需支持用户在设置里配的代理，Phase 1 评估 updater 的 `Builder` 是否暴露 proxy 配置，不支持则文档注明「更新检查走直连/系统代理」。

## Phase 1 摘要（详见 data-model.md / contracts / quickstart.md）

- **数据结构**：`latest.json`（updater 标准格式：`version` / `notes` / `pub_date` / `platforms.{target-arch}.{signature,url}`）由 CI 生成，客户端不手写。前端 `Update` 对象由插件返回，封装出 `{ version, currentVersion, body, date }` 供 UI 展示。
- **插件契约**：`check()` → `Update | null`；`update.downloadAndInstall(onEvent)` 带 `Started/Progress/Finished` 进度事件；安装后 `@tauri-apps/plugin-process` 的 `relaunch()` 重启。
- **前端 API 契约**：`update.api.ts` 收拢插件调用，组件不直接 import 插件（对齐既有「组件不直调底层」风格）。
- **发版基建步骤**：生成密钥 → 填 Secrets → 填公钥 → 改 workflow → 发一个基线 Release → 旧版本验证。详见 quickstart.md。

## 实施要点与风险

| 关注点 | 要点 | 回退方案 |
|---|---|---|
| **私钥管理** | `tauri signer generate` 生成，私钥仅入 GitHub Secrets 与用户本地安全备份，绝不入库 | 私钥丢失则存量用户无法验签后续更新——强制文档记录备份位置 |
| macOS bundle | updater 需 `.app.tar.gz`（非 `.dmg`）；需核对 `targets:"all"` 是否产出 | 若缺则在 release.yml 显式加 updater bundle 目标 |
| Linux bundle | `.deb` 不支持原地更新，仅 `.AppImage` 可 | endpoint 只挂 AppImage；deb 用户走手动下载，UI 文案说明 |
| Windows | `.msi`/`.nsis` 均可 updater，注意安装需退出应用 | 复用插件默认安装重启流程 |
| 未做 OS 签名 | 安装后仍弹系统警告 | 沿用 README 绕过说明，不阻断更新链路；不在本特性范围内做证书 |
| 首个基线版本 | 老版本必须已内置公钥+endpoint 才能检测到更新 | 本次改造后发的第一个 Release 即基线，之后才有更新可检测 |
| 检查失败 | 网络/限流/无 Release | 中文友好提示 + 失败不阻断（对齐 `loadLogStats` 策略） |
| 代理支持 | updater 是否走设置里的代理待 Phase 1 验证 | 不支持则文案注明「更新走系统网络」 |

## Complexity Tracking

| 违规项 | 为何必要 | 更简单方案为何被否 |
|---|---|---|
| 新增 4 个 Tauri 官方插件包（2 Rust + 2 前端），违反 002-plan「不新增第三方依赖」惯例 | 用户明确要求「应用内自动下载安装」，官方 updater 是唯一成熟方案 | 自建「检查+跳转下载」零依赖，但只能提示不能自动安装，不满足用户已确认的需求 |
| 引入 minisign 签名密钥管理与 release.yml 发版改造 | updater 强制验签，无签名链路无法安装更新 | 无更简方案——updater 的验签不可关闭，这是其安全模型的核心 |
