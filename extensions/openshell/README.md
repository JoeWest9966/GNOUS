# openshell · OpenShell 辅助面板

一个页面级扩展：在「扩展」分区新增 8 个视图、45 个区块，把 `openshell` CLI 的网关 / 沙箱 / 策略 / 凭据 /
观测能力收成一块工作面。选型过程见 `demo/openshell-panel/`（模块挑选器）。

## 页面结构

顶栏 tab 来自 manifest，进入任一 tab 时触发一次只读探测（`enter_openshell_page`），
之后可手动 `REFRESH`。探测在后台线程跑，UI 只翻 `busy`。

| View | 区块 | 主要数据来源 |
| --- | --- | --- |
| `Gateway` | 6：状态卡 / 切换器 / 认证身份 / 运行时 / 环境体检 / 指标条 | `status`、`gateway list`、`whoami`、`gateway info`、`doctor check` |
| `Sandboxes` | 7：总览表 / 创建向导 / 电源控制 / 单沙箱详情 / 模板库 / 工作区范围 / 回收站 | `sandbox list`、`sandbox get`、`sandbox template list` |
| `Inside` | 5：命令执行台 / 文件推拉 / SSH 片段 / 编辑器直通 / 终端接入 | `sandbox exec`、`sandbox ssh-config` |
| `Network` | 4：端口转发 / 服务隧道 / 服务暴露 / 端点清单 | `forward list|start|stop`、`service list|expose` |
| `Policy` | 5：策略总览 / 增量编辑 / 试算差异 / 策略历史 / 全局旋钮 | `policy get|list`、`policy update --dry-run`、`settings get --global` |
| `Credentials` | 7：Provider 列表 / 创建 / 附加矩阵 / Profile / 导入校验 / 轮换 / 凭据健康 | `provider list`、`sandbox provider list`、`profile list`、`provider refresh status` |
| `Observe` | 5：日志流 / 日志检索 / 设置编辑器 / 动作时间线 / 原始输出探针 | `logs`、`settings get --global`、面板动作日志 |
| `Tooling` | 6：命令生成器 / 批量动作 / TUI 直通 / 补全别名 / 机队矩阵 / 危险动作审计 | 静态命令模板 + `sandbox list` 聚合 |

## 真实状态约定

- 每个区块的 `cmd` 行显示它对应的真实命令；卡片内容是 stdout 的解析结果或原文。
- 命令失败（exit ≠ 0）时原样展示 stdout/stderr，不把失败画成绿灯。
- **不伪造不上报的字段**：网关不返回 CPU / 内存占用，`Sandbox Detail` 就不画资源仪表；
  `provider refresh` 没有到期日期，`Secret Health` 就不画倒计时；`policy get` 只给摘要，
  就没有端点逐条视图。
- 交互式命令（`connect`、`login`、`--editor`、`term`）只给可复制的命令，不在面板里代跑。
- 写操作分两级：普通写动作二次确认（同一个按钮点两次）；删除 / 解绑 / 停转发标 `DANGER`。
- `policy update` 只走 `--dry-run`，真正的 apply 留在终端。
- 动作日志保留最近 40 条，喂给「动作时间线」与「危险动作审计」。

## 代码

- `src/openshell.rs`：CLI 封装（`timeout` 保护 + stdout/stderr 合并）、快照 `probe()`、
  宽松 JSON 解析（不认识的输出返回空而不是假数据）、`Action` → argv 映射、动作执行与状态。
- `src/pages/openshell.rs`：8 个视图的渲染器，全部用 `block()` 卡片 + `kv` / `table` /
  `verbatim` / `chips` 原子拼装。
- 注册链路：`extensions/openshell/manifest.toml` → `registry::MANIFESTS` →
  `pages/mod.rs` → `main.rs` 的 `content()` 分派 + `App::run_openshell`。

## 已知边界

- 输入框（创建沙箱的 CPU / 内存 / 镜像、exec 的自定义命令）需要 GPUI 焦点管理，首版没做；
  参数从真实数据或固定只读预设里选。
- 批量动作只给命令预览，不执行（破坏性动作不进批量）。
- 面板不传 `-g` / `--workspace`，跟随 CLI 默认作用域；跨工作区统计需在终端加 `--all-workspaces`。
