# Quickstart：发版基建操作步骤 + 验收自测

**Feature**: 003-app-auto-update | **Phase 1** | 关联 [plan.md](../plan.md)

本文件分两部分：**A. 发布侧一次性基建**（部分需在 GitHub 后台操作，由维护者执行）；**B. 客户端验收自测**（编码完成后 GUI 实测清单）。

---

## A. 发布侧一次性基建

> ⚠️ 顺序不可颠倒：先有签名密钥与发版清单，客户端更新才能验签通过。

### A1. 生成 updater 签名密钥对（本地一次性）

```bash
# 在项目根执行；-w 指定私钥落盘位置（勿放进仓库目录）
npm run tauri signer generate -- -w ~/.gitview/updater.key
# 交互设置密钥密码（记牢，CI 需要）；产出：
#   ~/.gitview/updater.key      （私钥，绝不入库、绝不外泄）
#   ~/.gitview/updater.key.pub  （公钥，填入 tauri.conf.json）
```

- **私钥备份**：复制到密码管理器 / 离线安全位置。**私钥丢失 = 所有存量用户无法再验签任何后续更新**，只能让用户手动重装。

### A2. 私钥入 GitHub Actions Secrets（GitHub 后台）

仓库 → Settings → Secrets and variables → Actions → New repository secret：

| Secret 名 | 值 |
|---|---|
| `TAURI_SIGNING_PRIVATE_KEY` | `~/.gitview/updater.key` 文件内容（整段） |
| `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` | A1 设置的密钥密码 |

### A3. 公钥填入 `tauri.conf.json`

```jsonc
"plugins": {
  "updater": {
    "endpoints": [
      "https://github.com/menglavol/git-view/releases/latest/download/latest.json"
    ],
    "pubkey": "<~/.gitview/updater.key.pub 文件内容>"
  }
}
```

### A4. 改造 `.github/workflows/release.yml`

- `tauri-action` 步骤注入签名环境变量（检测到即自动生成 `.sig` 与 `latest.json`）：
  ```yaml
  env:
    TAURI_SIGNING_PRIVATE_KEY: ${{ secrets.TAURI_SIGNING_PRIVATE_KEY }}
    TAURI_SIGNING_PRIVATE_KEY_PASSWORD: ${{ secrets.TAURI_SIGNING_PRIVATE_KEY_PASSWORD }}
  ```
- 核对各平台产出 updater 兼容 bundle：macOS `.app.tar.gz`、Windows `.msi`/`.nsis`、Linux `.AppImage`。确认 `latest.json` 出现在 Release assets。

### A5. 发一个基线 Release

- 打 tag（如 `v0.1.1`）触发 workflow；确认 Release 含各平台包 + `.sig` + `latest.json`。
- **注意**：本次改造前发布的旧版本不含公钥/endpoint，无法检测更新；只有本次改造后发出的版本，及其之后的版本，才具备被更新检测的能力。

---

## B. 客户端验收自测（GUI 实测清单）

> 沿用项目前端协作流程：门禁全绿后由用户 GUI 实测，实测通过再提交。

### B1. 静态门禁
```bash
pnpm run lint && pnpm run format:check
pnpm run rust:fmt && pnpm run rust:clippy
pnpm run check:comment-ratio
pnpm run check:no-debug-prints
```

### B2. 功能实测（需两个版本：本地低版本 + 已发布高版本 Release）

| 场景 | 操作 | 预期 |
|---|---|---|
| 已是最新 | 当前版本 = 最新 Release，点「检查更新」 | info 提示「已是最新版本」 |
| 发现新版 | 当前版本 < 最新 Release，点「检查更新」 | success 提示新版本号 + 发布说明可展开 |
| 下载进度 | 点「下载并安装」 | 进度条实时增长，显示已下载/总量 |
| 验签安装 | 下载完成 | 无验签错误，进入安装 |
| 重启生效 | 安装后确认「立即重启」 | 应用重启，版本号变为新版本 |
| 检查失败 | 断网后点「检查更新」 | warning 中文提示，页面其余功能正常 |
| 稍后重启 | 安装后选「稍后」 | 不重启，下次启动生效 |

### B3. 跨平台确认
- macOS / Windows：确认安装后系统安全警告可按 README 说明绕过（预期内，不算缺陷）。
- Linux：确认走 `.AppImage` 路径可更新；`.deb` 用户提示手动下载。

### B4. 提交
- 门禁全绿 + 上述实测通过后，按关注点分组提交（基建配置 / 前端能力 / 文档分开）。
