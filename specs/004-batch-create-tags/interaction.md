# 页面交互和状态流转设计：批量创建 Tag

## 页面入口

远程仓库页 [RemoteRepositories.vue](/Volumes/venus/code/git-view/src/pages/RemoteRepositories.vue) 的顶部新增“批量添加 Tag”按钮。

- 树形视图：目录勾选会级联选中子项目；父页面接收真实项目列表。
- 列表视图：逐项目勾选。
- 没有选中项目时按钮禁用。
- 筛选、同步或切换视图造成项目集合改变时，清理不再存在的选择，并保留仍存在的选择。

## 配置页面

组件建议：`src/components/repository/BatchCreateTagDialog.vue`。

### 顶部区域

展示项目数量、并发数和操作按钮：

```text
已选项目：N 个       并发数：[3]（范围 1～20）
[添加项目] [移除选中] [执行预检查]
```

“添加项目”打开项目选择器。项目选择器使用当前远程仓库数据；用户可以清除筛选后选择全部项目。已添加项目在选择器中保持选中，避免重复添加。

### 项目配置表

每行包含：

```text
项目全名 | 平台 | 账号 | 分支 | 预检查提交 | Tag 类型 | Tag 名称 | 附注说明 | 操作
```

分支控件：

- 默认值为 `defaultBranch`；
- 打开时惰性请求分支列表；
- `filterable` 支持输入关键字筛选；
- 只允许从服务端返回的分支中选择；
- 选择后异步加载分支 head commit；
- 显示短 SHA，完整 SHA 和提交描述通过 tooltip；
- 加载失败时该项目预检查失败，不静默使用其他分支。

Tag 类型控件：轻量 / 附注，默认轻量。切换为附注后显示该行独立多行说明输入框，并校验 1～2000 字符。

Tag 名称控件：初始为空或取用户输入的统一名称；提供“统一设置”输入框和“应用到全部”按钮，应用后仍允许逐行覆盖。

## 状态机

```text
idle
  └─ open → editing
editing
  ├─ edit project/branch/tag → editing
  ├─ run precheck → prechecking
  └─ cancel → closed
prechecking
  ├─ any invalid item → needs_fix
  └─ all valid → confirmation
needs_fix
  ├─ fix and recheck → prechecking
  ├─ remove invalid items → editing
  └─ cancel → closed
confirmation
  ├─ back → editing
  ├─ confirm → executing
  └─ cancel → closed
executing
  ├─ all finished → result
  └─ cancel remaining → cancelling → result
result
  ├─ retry failures → prechecking
  ├─ close → closed
  └─ view logs → Logs page / existing log view
```

## 预检查页面

预检查结果按项目展示：

- 可创建：绿色，显示分支 SHA、Tag 名称和类型；
- 失败：红色，显示错误分类和修复建议；
- 检查中：加载状态。

失败项目包括：分支不存在、无法读取分支 head、Tag 已存在、Tag 名称非法、附注说明缺失、账号权限不足、平台不支持对应 Tag 类型等。

只要存在失败项目：

- “进入确认”按钮禁用；
- 提供“移除失败项目”；
- 用户修改后可只重检修改项目，也可全部重检。

## 确认摘要

摘要逐项目展示：

```text
项目 | 平台 | 账号 | 分支 | 预检查 SHA | Tag 类型 | Tag 名称 | 附注说明
```

顶部展示总数、轻量/附注数量、不同分支数量和并发数。预检查 SHA 使用完整值存储，界面显示短 SHA，悬停显示完整值。

确认时再次检查当前配置未被修改；配置变化则返回编辑状态并要求重检。

## 执行与取消

执行页展示：

```text
总计 N   成功 S   失败 F   取消 C   等待 W
当前并发：K / 20
```

每个项目状态：等待、创建中、成功、失败、取消。取消操作只阻止等待队列，已发送请求继续等待结果。

## 结果页

结果表展示项目、分支、Tag、状态、预检查 SHA、实际创建 SHA、错误信息和耗时。

如果实际创建 SHA 与预检查 SHA 不同，显示“分支在执行期间发生变化，已按执行时最新提交创建”。

当存在失败项目，显示“重试失败项目”按钮。重试会筛选失败项、重新加载分支和提交、重新预检查，并再次打开确认摘要。
