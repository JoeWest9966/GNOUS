# root-align · Root 桌面对齐

一个页面级扩展，围绕 `/home/root/GNOME/Root用户桌面功能对齐/root-gnome-align.sh` 做「诊断台」（选型方案 C）。

## 页面结构

- 顶部：`ALIGNED / ATTENTION / FAILED / UNKNOWN` 结论、8 个单元覆盖数、系统文件改动数；按钮 `STATUS / DOCTOR / APPLY / REVERT`
- 左侧：音频 / 设备枚举 / 媒体控制 / 全局搜索四个功能域，逐项列出单元真实状态
- 右侧：`STATUS`、`DOCTOR`、`APPLY LOG` 三个 tab，外部命令 stdout 原样显示
- 底部：`GNOUS_FIXES_ROOT` 工具目录、`state.json` 只读路径、`restart-shell` 独立危险区

## 真实状态约定

| 动作 | 外部命令 | 副作用 |
| --- | --- | --- |
| `STATUS` | `./root-gnome-align.sh status` | 只读 |
| `DOCTOR` | `./root-gnome-align.sh doctor` | 只读（读 systemd 日志） |
| `APPLY` | `./root-gnome-align.sh apply` | 写 `~/.config/systemd/user/*.d/override.conf`，二次确认 |
| `REVERT` | `./root-gnome-align.sh revert` | 删除覆盖与 `state.json`，二次确认 |
| `RESTART SHELL` | `./root-gnome-align.sh restart-shell` | 会打断 GNOME 会话，独立危险区 + 二次确认 |

- 所有状态来自 stdout 解析，工具缺失 / 输出无法解析时显示失败原因，绝不伪造绿灯。
- `APPLY / REVERT` 成功后自动重跑 `STATUS`，动作日志仍留在 `APPLY LOG`。
- `RESTART SHELL` 永不进入批量动作；危险 flags 不从 UI 触发。
- 与 `gnome-fixes` 共用 `GNOUS_FIXES_ROOT`（默认 `/home/root/GNOME`），无需第二套根目录配置。

## 与 gnome-fixes 的关系

`gnome-fixes` 里的 `rootdesk` 卡片继续保留，提供「体检 / 应用 / 回退」快捷入口；本页是同一工具的深入诊断视图，两者都调用同一个外部脚本，不复制脚本逻辑。
