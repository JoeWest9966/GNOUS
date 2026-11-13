# root-align · Root 桌面对齐

一个页面级扩展，基于 GNOUS 内建 `root-align` 后端做「诊断台」（选型方案 C）。

## 页面结构

- 顶部：`ALIGNED / ATTENTION / FAILED / UNKNOWN` 结论、8 个单元中健康数、系统级文件改动数；按钮 `STATUS / DOCTOR / APPLY / REVERT`
- 左侧：音频 / 设备枚举 / 媒体控制 / 全局搜索四个功能域，逐项列出单元真实状态
- 右侧：`STATUS`、`DOCTOR`、`APPLY LOG` 三个 tab，内建后端 stdout 原样显示
- 底部：`GNOUS_STATE_HOME` 后端目录、JSON 状态文件只读路径、`restart-shell` 独立危险区

## 真实状态约定

| 动作 | 内建操作 | 副作用 |
| --- | --- | --- |
| `STATUS` | `gnous builtin root-align / status` | 只读；返回 8 个单元与音频设备的结构化 facts |
| `DOCTOR` | `gnous builtin root-align / doctor` | 只读（读 `/usr/lib/systemd/user` 条件与 journalctl） |
| `APPLY` | `gnous builtin root-align / apply` | 写 `/root/.config/systemd/user/*.d/override.conf`，二次确认 |
| `REVERT` | `gnous builtin root-align / revert` | 按 JSON 状态精确停止 / 删除覆盖，二次确认 |
| `RESTART SHELL` | `gnous builtin root-align / restart-shell` | 会打断 GNOME 会话，独立危险区 + 二次确认 |

- 健康结论来自 `Outcome.facts.status` 与逐单元 `healthy` 字段，不解析 stdout 关键字；事实缺失就是 `UNKNOWN`，失败就是 `FAILED`。
- `APPLY / REVERT` 成功后自动重跑 `STATUS`，动作日志仍留在 `APPLY LOG`。
- `APPLY` 只写 root 用户目录，不改 `/usr/lib/systemd/user`，不 enable/disable 全局单元，joe 会话不受影响。
- 旧 `/root/.config/root-gnome-align/state.json` 会在首次用到状态时迁移为 `~/.local/state/gnous/fixes/root-align.json`；旧外部工具目录不再被调用，可删除。
- 所有动作写 `~/.local/state/gnous/fixes/audit.jsonl`；`RESTART SHELL` 永不进入批量动作。

## 与 gnome-fixes 的关系

`gnome-fixes` 里的 `rootdesk` 卡片继续提供「体检 / 应用 / 回退」快捷入口；本页是同一内建后端的深入诊断视图，两者共用 `fixes_backend::run`，不复制逻辑。
