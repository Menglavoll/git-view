# Phase 1 数据模型：应用内自动更新

**Feature**: 003-app-auto-update | **Date**: 2026-07-13

本特性**不涉及 SQLite / keyring 变更**，无数据库迁移。数据结构集中在两处：CI 生成的更新清单 `latest.json`（客户端只读、不手写），与前端展示用的更新信息对象。

## 1. `latest.json` —— updater 标准更新清单（CI 生成）

由 `tauri-action` 在发版时自动生成并上传为 GitHub Release 资产，客户端从 endpoint 拉取。**客户端不生成、不修改此文件**，此处仅记录其结构以便理解链路。

```jsonc
{
  "version": "0.2.0",                       // 语义化版本，供 updater 与当前 app 版本比对
  "notes": "本次更新内容……",                // 发布说明，前端展示
  "pub_date": "2026-08-01T00:00:00Z",       // 发布时间（RFC3339）
  "platforms": {
    // key 为 "{target}-{arch}"，值含下载地址与该包的 minisign 签名
    "darwin-aarch64": {
      "signature": "<minisign 签名内容>",
      "url": "https://github.com/menglavol/git-view/releases/download/v0.2.0/GitView_aarch64.app.tar.gz"
    },
    "darwin-x86_64":   { "signature": "...", "url": "..." },
    "windows-x86_64":  { "signature": "...", "url": "...GitView_x64-setup.nsis.zip" },
    "linux-x86_64":    { "signature": "...", "url": "...GitView_amd64.AppImage.tar.gz" }
  }
}
```

**要点**：
- `signature` 由 CI 用私钥签名，客户端用内置公钥（`tauri.conf.json` 的 `plugins.updater.pubkey`）验签。
- `platforms` 的 key 由 updater 按运行平台/架构自动匹配；缺某平台条目 = 该平台无更新可用。
- Linux 仅挂 `.AppImage`（`.deb` 不支持原地更新）。

## 2. 前端更新信息对象（插件返回，UI 展示用）

`@tauri-apps/plugin-updater` 的 `check()` 返回 `Update | null`。`update.api.ts` 将其规约为一个精简、稳定的视图对象供组件消费：

```typescript
/** 检查结果：无更新时为 null，有更新时含展示所需字段。 */
export interface UpdateInfo {
  /** 可更新到的目标版本（如 "0.2.0"） */
  version: string;
  /** 当前应用版本（如 "0.1.0"），从 tauri.conf.json 读取 */
  currentVersion: string;
  /** 发布说明（latest.json 的 notes，可能为空） */
  body?: string;
  /** 发布时间（latest.json 的 pub_date，可能为空） */
  date?: string;
}

/** 下载进度事件（downloadAndInstall 回调），供进度条渲染。 */
export interface DownloadProgress {
  /** 已下载字节数 */
  downloaded: number;
  /** 总字节数（部分服务器不返回 Content-Length 时可能为 0） */
  contentLength: number;
}
```

**设计说明**：
- 不直接把插件的 `Update` 类型透传给组件，避免组件耦合插件内部结构；`update.api.ts` 做一层收拢与字段挑选（对齐项目「组件不直调底层」的既有分层）。
- `currentVersion` 用于 UI 始终展示「当前版本」，即便无更新也需显示。
- 无独立「无更新」结构：`check()` 返回 `null` 即表示已是最新，前端据此展示 info 提示。

## 3. 状态（前端本地，不入 store）

更新交互状态用 `AboutUpdate.vue` 组件内 `ref` 管理，**不污染 settings store**（与设置页「本地副本」哲学一致）：

| 状态 | 类型 | 含义 |
|---|---|---|
| `currentVersion` | `string` | 当前应用版本，挂载时经 `getVersion()` 读取 |
| `checking` | `boolean` | 检查更新进行中，禁用按钮 |
| `updateInfo` | `UpdateInfo \| null` | 检查结果；null 且已检查过 = 已是最新 |
| `checked` | `boolean` | 是否已执行过检查（区分「未检查」与「已检查无更新」） |
| `downloading` | `boolean` | 下载安装进行中 |
| `progress` | `DownloadProgress \| null` | 下载进度，渲染进度条 |
| `errorMsg` | `string \| null` | 检查/下载失败的脱敏错误信息 |

## 关系图

```
CI 发版 ──生成──► latest.json (GitHub Release 资产)
                       │
              endpoint 拉取 + 公钥验签
                       ▼
   updater.check() ──► Update | null
                       │
        update.api.ts 规约为 UpdateInfo
                       ▼
              AboutUpdate.vue (ref 状态 + UI)
                       │
     downloadAndInstall(onProgress) ──► DownloadProgress 事件
                       ▼
              process.relaunch() 重启生效
```
