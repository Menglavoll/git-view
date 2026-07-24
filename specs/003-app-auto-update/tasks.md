# Tasks: 应用内自动检查与更新

**Input**: Design documents from `/specs/003-app-auto-update/`
**Prerequisites**: plan.md（必需）、spec.md（用户故事来源）、research.md、data-model.md、contracts/updater-flow.md、quickstart.md

**Tests**: 本特性主逻辑落在 `tauri-plugin-updater` 前端插件侧，后端几乎无新逻辑，无契约测试。仅在 Polish 阶段做一次真实发版 + 旧版本端到端验证（见 quickstart.md）。不生成单独的自动化测试任务（沿用项目「解析类纯函数才加 cargo test」的既定惯例，本特性无此类逻辑）。

**Organization**: 任务按用户故事分组。但本特性的关键前置是「发布侧签名基建」——它不属于任何单一用户故事，而是**所有客户端更新能力的阻塞前提**，故归入 Phase 2 Foundational。

## Format: `[ID] [P?] [Story?] Description`

- **[P]**：可并行（不同文件、无未完成依赖）
- **[Story]**：所属用户故事（US1 / US2 / US3）；Setup / Foundational / Polish 阶段无 Story 标签
- 每个任务标注确切文件路径

## Path Conventions

- 前端：仓库根 `src/`
- 后端：`src-tauri/`
- 发布基建：`src-tauri/tauri.conf.json`、`.github/workflows/release.yml`、GitHub Secrets（用户在后台操作）

---

## Phase 1: Setup（共享基础）

**Purpose**：安装依赖、注册插件、授予权限——三个用户故事共用的客户端底座。

- [X] T001 在 `src-tauri/Cargo.toml` 的 `[dependencies]` 新增 `tauri-plugin-updater = "2"` 与 `tauri-plugin-process = "2"`，并加中文注释说明用途（应用内更新 / 更新后重启），依赖新增理由记入本特性 Complexity Tracking
- [X] T002 在 `package.json` 的 `dependencies` 新增 `@tauri-apps/plugin-updater` 与 `@tauri-apps/plugin-process`（版本与 `@tauri-apps/api` 对齐 ^2），运行 `pnpm install` 更新 `pnpm-lock.yaml`
- [X] T003 在 `src-tauri/src/lib.rs` 的 `tauri::Builder` 链上注册两个插件：`.plugin(tauri_plugin_updater::Builder::new().build())` 与 `.plugin(tauri_plugin_process::init())`，紧邻现有 `.plugin(...)` 调用，加中文注释
- [X] T004 在 `src-tauri/capabilities/default.json` 的 `permissions` 数组新增 `"updater:default"` 与 `"process:allow-restart"`，并更新该文件顶部 `description` 说明新增权限用途（对齐「按 User Story 增量追加 command 白名单」的既定策略）

**Checkpoint**：`pnpm run tauri:dev` 能编译启动，插件已加载（此阶段尚无 UI 变化）。

---

## Phase 2: Foundational（阻塞前提 —— 发布侧签名基建）

**Purpose**：生成签名密钥、配置发版签名链路、产出 `latest.json`。**这是所有客户端更新能力的阻塞前提**——没有带签名的 Release 与内置公钥，`check()` 无法验签任何更新。部分步骤需用户在本机 / GitHub 后台操作（我无法接触私钥与 Secrets）。

**⚠️ CRITICAL**：在完成 T005–T009 并成功发出「第一个带签名的基线 Release」之前，US1/US2/US3 的客户端行为无法端到端验证。

- [ ] T005 [用户执行] 本机运行 `pnpm tauri signer generate -w ~/.gitview-updater.key` 生成 minisign 密钥对；将**私钥内容**与**私钥密码**分别配置为 GitHub 仓库 Secrets `TAURI_SIGNING_PRIVATE_KEY` 与 `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`；私钥另行安全备份（丢失将导致存量用户无法验签后续更新）。步骤详见 `specs/003-app-auto-update/quickstart.md`
- [X] T006 在 `src-tauri/tauri.conf.json` 新增 `plugins.updater` 配置：`endpoints` 指向 `https://github.com/menglavol/git-view/releases/latest/download/latest.json`，`pubkey` 填入 T005 生成的**公钥**内容；加注释说明 endpoint 为 GitHub 稳定跳转、无需自建服务器
- [X] T007 改造 `.github/workflows/release.yml`：在 `tauri-action` 步骤的 `env` 注入 `TAURI_SIGNING_PRIVATE_KEY` 与 `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`（引用 T005 的 Secrets），使发版时自动为产物生成 `.sig` 签名并产出 `latest.json`
- [X] T008 在 `.github/workflows/release.yml` 核对/调整各平台 updater 兼容 bundle：macOS 需产出 `.app.tar.gz`（非仅 `.dmg`）、Windows `.msi`/`.nsis`、Linux `.AppImage`（`.deb` 不支持原地更新）；确认 `tauri.conf.json` 的 `bundle.targets` 与 `createUpdaterArtifacts` 设置能产出上述格式
- [X] T009 在 `.github/workflows/release.yml` 顶部注释补充「更新签名链路」说明（私钥来源、latest.json 产出、公钥位置），与既有「发布门禁」注释风格一致

**Checkpoint**：具备发版签名能力。真实发版验证放在 Polish 阶段（需先合入客户端代码，产出可作基线的版本）。

---

## Phase 3: User Story 1 — 检查是否有新版本（Priority: P1）🎯 MVP

**Goal**：设置「通用」Tab 底部展示当前版本，点击「检查更新」后请求 GitHub Releases 并回显「已是最新」或「发现新版本 vX.Y.Z」。

**Independent Test**：打开设置「通用」Tab 底部「关于与更新」分区，见当前版本号；点「检查更新」，数秒内得到「已是最新」/「发现新版本」/失败提示之一。

- [ ] T010 [P] [US1] 新建 `src/api/update.api.ts`：封装 `checkForUpdate()`（调 `@tauri-apps/plugin-updater` 的 `check()`，返回规约后的 `{ available, version, currentVersion, body, date }` 或 null），组件不直接 import 插件；加中文文档注释说明「为何收拢到 api 层」（对齐既有「组件不直调底层」风格）。契约见 `contracts/updater-flow.md`
- [ ] T011 [P] [US1] 在 `src/i18n/zh.ts` 的 `settings` 下新增 `about`/`update` 文案键：`about`（分区标题「关于与更新」）、`currentVersion`、`checkUpdate`、`checking`、`upToDate`、`hasUpdate`（含 `{version}` 占位）、`checkFailed`；保持与既有 `settings.storage.*` 文案风格一致
- [ ] T012 [P] [US1] 在 `src/i18n/en.ts` 同步 T011 的英文文案键，键名与结构与 zh.ts 完全对齐
- [ ] T013 [US1] 新建 `src/components/settings/AboutUpdate.vue`：展示当前版本（`@tauri-apps/api/app` 的 `getVersion()`）、「检查更新」按钮（`checking` loading 态）、检查结果回显（复用 `el-alert`：`upToDate` info / `hasUpdate` success / 失败 warning，风格对齐「检测 Git」回显）；检查逻辑调 T010 的 `update.api.ts`，失败仅提示不抛（对齐 `loadLogStats` 策略）。本任务只实现 US1 的「检查+回显」，下载安装留待 US2
- [ ] T014 [US1] 在 `src/pages/Settings.vue` 通用 Tab 底部「日志与存储」分区之后，新增 `<el-divider content-position="left">` +「关于与更新」分区，引入并挂载 `AboutUpdate` 子组件（仅新增约 10 行，保持 Settings.vue 不超 500 行门禁）

**Checkpoint**：US1 可独立验证——能看到当前版本、能检查并得到明确结果。若已有更高版本的基线 Release，可见「发现新版本」。

---

## Phase 4: User Story 2 — 下载并安装新版本（Priority: P1）

**Goal**：在「发现新版本」状态下点击「下载并安装」，显示实时进度，下载完成后验签、安装并引导重启。

**Independent Test**：在「发现新版本」状态点「下载并安装」→ 见进度条推进 → 安装完成 → 引导重启 → 重启后版本号更新。

- [ ] T015 [US2] 在 `src/api/update.api.ts` 扩展 `downloadAndInstall(update, onProgress)`：封装 `update.downloadAndInstall(onEvent)`，把 `Started`（总字节）/`Progress`（累计字节）/`Finished` 事件归一为进度回调；再封装 `relaunch()`（调 `@tauri-apps/plugin-process` 的 `relaunch()`）。契约见 `contracts/updater-flow.md`
- [ ] T016 [US2] 在 `src/components/settings/AboutUpdate.vue` 扩展「发现新版本」状态：新增「下载并安装」按钮与 `el-progress` 进度条；点击调 T015 的 `downloadAndInstall`，用进度回调驱动进度条（百分比不倒退）；下载/验签失败给中文友好提示并允许重试，不留半成品（对齐 spec 边界要求）
- [ ] T017 [US2] 在 `AboutUpdate.vue` 安装完成后引导重启：复用 `ElMessageBox.confirm` 交互范式（参考 `Settings.vue` 的 `promptRestart`），「立即重启」调 T015 的 `relaunch()`、「稍后」关闭提示；`closeOnClickModal: false` 强制显式选择

**Checkpoint**：US1 + US2 均可独立工作——检查到新版后可一键下载安装并重启（依赖已有一个更高版本的签名 Release，见 Polish 的真实发版验证）。

---

## Phase 5: User Story 3 — 查看新版本的更新内容（Priority: P2）

**Goal**：「发现新版本」时展示该版本发布说明，内容与 GitHub Release 一致，过长可折叠/滚动。

**Independent Test**：在「发现新版本」状态，「关于与更新」分区展示 release notes；内容超长时可折叠或内部滚动，不撑破布局。

- [ ] T018 [P] [US3] 在 `src/i18n/zh.ts` 与 `src/i18n/en.ts` 的 `settings.update` 下新增 `releaseNotes`（「更新内容」标题）与 `noReleaseNotes`（无说明时占位）文案键
- [ ] T019 [US3] 在 `src/components/settings/AboutUpdate.vue` 的「发现新版本」区块，用 `el-collapse` 或带 `max-height + overflow:auto` 的容器展示 `check()` 返回的 `body`（release notes）；纯 CSS 折叠/滚动，不引虚拟滚动，不破坏 US1/US2 已有交互

**Checkpoint**：三个用户故事均可独立功能验证。

---

## Phase 6: Polish & Cross-Cutting Concerns

**Purpose**：跨故事收尾、质量门禁、真实发版端到端验证。

- [X] T020 运行质量门禁：`pnpm run lint`、`pnpm run format:check`、`pnpm run rust:fmt`、`pnpm run rust:clippy`、`pnpm run check:comment-ratio`（新增/改动文件中文注释 ≥ 0.3，不足补注释而非豁免）、`pnpm run check:no-debug-prints`，全绿
- [X] T021 [P] 更新 `docs/roadmap.md`：在 V1 能力表补「应用内自动更新」一行（或标注版本归属），与实际交付对齐
- [X] T022 [P] 更新 `README.md`「安装」章节：说明应用内更新能力与「未做 OS 代码签名仍有首次运行警告」的现状衔接
- [ ] T023 [用户执行] 真实发版验证：合入客户端代码后，打 `v*` tag 触发 `release.yml`，确认产物含 `.sig` 与 `latest.json`；用一个**更低版本**的旧客户端验证能检测到更新、下载验签安装、重启后版本更新（端到端跑通 US1+US2+US3）。步骤见 `specs/003-app-auto-update/quickstart.md`
- [ ] T024 待用户完成 GUI 实测（T023）后，按关注点分组提交（基建配置 / 前端能力 / 文档），遵循 [[feedback_frontend_workflow]] 既定协作流程

---

## Dependencies & Execution Order

### 阶段依赖

- **Setup（Phase 1）**：无前置，最先做。
- **Foundational（Phase 2）**：依赖 Setup（T003/T004 的插件与权限需先就位再配置 endpoint/公钥）；**阻塞所有用户故事的端到端验证**（无签名 Release 则 `check()` 无更新可查）。但 US1–US3 的**客户端代码编写**不依赖 Phase 2 完成，可并行推进，只是「联调验证」须等 T023 的基线 Release。
- **US1（Phase 3）**：依赖 Setup。MVP。
- **US2（Phase 4）**：依赖 US1（复用 `AboutUpdate.vue` 的「发现新版本」状态与 `update.api.ts`）。
- **US3（Phase 5）**：依赖 US1（在同一「发现新版本」区块加展示）；与 US2 相对独立，可并行。
- **Polish（Phase 6）**：依赖全部实现完成。

### 用户故事独立性

- US1 可独立交付并验证（检查 + 回显），即 MVP。
- US2 在 US1 之上增量（下载/安装/重启）。
- US3 在 US1 之上增量（发布说明展示），不依赖 US2。

---

## Parallel Execution Examples

**Setup 阶段**（不同文件，可并行）：

- T001（Cargo.toml）、T002（package.json）可并行；T003/T004 依赖 T001 到位后进行。

**US1 阶段起步**（不同文件，可并行）：

- T010（update.api.ts）、T011（zh.ts）、T012（en.ts）三者并行；
- T013（AboutUpdate.vue）依赖 T010/T011/T012；T014 依赖 T013。

**Polish 阶段**：

- T021（roadmap）、T022（README）可并行；T020 门禁在代码定稿后跑；T023/T024 由用户执行且有先后。

---

## Implementation Strategy

### MVP 优先

1. 完成 **Setup（T001–T004）** → 客户端底座就位。
2. 完成 **US1（T010–T014）** → 可检查更新并回显，即最小可用增量。
3. 并行推进 **Foundational 发布基建（T005–T009）** → 为真实更新验证铺路（用户执行部分尽早启动）。
4. 增量交付 **US2（T015–T017）** 与 **US3（T018–T019）**。
5. **Polish（T020–T024）** 收尾：门禁 + 文档 + 真实发版端到端验证 + 分组提交。

### 关键提醒

- **私钥安全**（T005）：私钥丢失=存量用户无法再更新，务必备份并记录位置。
- **首个基线**（T023）：本次改造后发的第一个签名 Release 才是「可被检测到更新」的起点，之前版本无法感知。
- **未做 OS 签名**：安装后系统警告属已知现状，不在本特性消除范围。

---

## Format Validation

- [x] 每个任务行以 `- [ ]` 开头
- [x] 每个任务有顺序 ID（T001…T024）
- [x] `[P]` 仅标注真正独立（不同文件、无未完成依赖）的任务
- [x] 用户故事阶段任务均带 `[US1]/[US2]/[US3]` 标签；Setup/Foundational/Polish 无 Story 标签
- [x] 每个实现任务标注确切文件路径（用户执行类任务标注操作位置与 quickstart 引用）
- [x] 任务行匹配 `- [ ] [TaskID] [P?] [Story?] 描述 + 文件路径`
