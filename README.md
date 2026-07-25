# GitView

> 轻量级跨平台 Git 可视化客户端 —— 多平台多账号统一管理、远程仓库批量克隆、日常 Git 操作可视化。

GitView 是一个支持 **GitHub / GitLab / Gitee**(含自建 GitLab 实例)多平台多账号管理的桌面客户端，聚焦三件事：把分散在多个平台的仓库**统一浏览与搜索**、**多选批量克隆**、以及在本地仓库里完成**查看变更 → diff → 暂存 → 提交 → 推送**的可视化工作流。基于 Tauri 2 + Vue 3 + Rust 构建，覆盖 macOS / Windows / Ubuntu。

> 截图待补：账号管理 · 远程仓库中心 · 批量克隆 · 单仓库工作区。

## 核心能力

- **多平台多账号统一管理**：GitHub / GitLab / Gitee 与自建 GitLab，Personal Access Token 验证连接；Token 存入操作系统原生密钥库，绝不明文落库或写日志。
- **远程仓库统一浏览与搜索**：一次同步、统一列表，按平台 / 账号 / 可见性 / 收藏筛选，关键词搜索，5000+ 条虚拟滚动。
- **多仓库批量克隆**(核心差异化)：多选仓库、可控并发(默认 3，上限 8)、实时进度、取消 / 重试、按目录策略组织、成功后自动纳入本地仓库。
- **本地仓库集中管理**：手动添加或扫描父目录批量纳入，状态总览(ahead/behind、未提交变更)，批量 Fetch，打开目录 / 终端。
- **单仓库可视化 Git 工作流**：文件变更、diff 高亮、stage/unstage、commit、fetch/pull/push、分支查看与切换、提交历史与提交详情（逐文件 diff）。
- **操作日志与诊断**：全量 Git 操作日志，敏感信息脱敏，常见错误中文友好提示。
- **设置中心**：Git 环境、默认目录、克隆协议与并发、主题、网络代理统一配置，以及应用数据目录迁移与日志清理。

更完整的逐功能操作说明见 [用户使用指南](docs/user-guide.md)；版本规划见 [路线图](docs/roadmap.md)。

## 安装

> V1 暂未做 Apple 公证与 Windows 代码签名，下载预编译包首次运行会触发系统安全警告，按下方说明放行即可；也可自行从源码构建。

### 预编译包(发布后)

从项目 Releases 下载对应平台安装包：

- **macOS**：`.dmg`，双击拖入「应用程序」。首次运行若提示「无法验证开发者」，**右键应用图标 → 打开 → 确认**。
- **Windows**：`.msi`，双击安装。SmartScreen 警告时点 **「更多信息」→「仍要运行」**。
- **Linux**：`.deb` 或 `.AppImage`。若添加账号时报「凭据存储不可用」，请先安装 `gnome-keyring` 或 `libsecret`。

### 系统前置依赖

- **Git**：GitView 依赖系统 Git CLI 执行实际 Git 操作，**不内置 Git**。未检测到时应用会进入引导页指导安装。
  - macOS：`brew install git` 或 `xcode-select --install`
  - Windows：从 [git-scm.com](https://git-scm.com/download/win) 下载安装
  - Linux：`sudo apt install git`(Debian/Ubuntu)等
- **凭据存储**：依赖 OS 原生密钥库(macOS Keychain / Windows Credential Manager / Linux Secret Service)。Linux 桌面需 `gnome-keyring` 或 `libsecret`。

## 开发

```bash
# 安装前端依赖(本仓库使用 pnpm-lock，亦可用 pnpm)
pnpm install

# 启动开发模式(Tauri 窗口 + Vite 热更新)
pnpm run tauri:dev
```

要求：Node 20+、Rust stable(1.75+)、各平台 Tauri 构建前置(见下)。

## 构建

```bash
# 产出当前平台的可分发安装包(macOS .dmg / Windows .msi / Linux .deb/.AppImage)
pnpm run tauri:build
```

跨平台构建前置：

- **macOS**：Xcode Command Line Tools。
- **Windows**：MSVC C++ Build Tools / Visual Studio 2022 Build Tools 与 WebView2 Runtime。
- **Linux**：`libwebkit2gtk-4.1-dev`、`libssl-dev`、`libgtk-3-dev`、`libayatana-appindicator3-dev`、`librsvg2-dev`。

三平台安装包由 `tag v*` 触发的 GitHub Actions(`.github/workflows/release.yml`)自动构建并上传到 Release。

## 测试与质量门禁

```bash
# 后端单元 / 集成测试
pnpm run rust:test

# 一键质量门禁(lint + 格式 + clippy + 中文注释比例 + 无调试输出)
pnpm run check:all

# 安全扫描(Token 明文 / 凭据残留)
pnpm run check:no-token-leak
bash scripts/check-credential-cleanup.sh
```

本项目遵循「中文注释比例 ≥ 0.3」「无遗留调试输出」「Token 零明文」等硬性门禁(见 `.specify/memory/constitution.md`)，CI 在合并前强制校验。

## 技术栈

- **桌面容器**：Tauri 2
- **前端**：Vue 3 + TypeScript + Vite + Element Plus + Pinia + Vue Router
- **后端**：Rust + tokio + reqwest(rustls) + rusqlite(SQLite) + keyring + tracing
- **实际 Git 操作**：系统 Git CLI

## 许可证

待定(TBD)。
