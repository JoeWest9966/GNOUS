# gnome-fixes · 系统修复

一个页面级扩展，把 `/home/root/GNOME/` 下那几个零散的小工具收成一个扩展。

## 设计

五项修复共用一个骨架：**体检 → 应用 → 回退**。每项都是一行卡片，点开看证据矩阵、自检日志和将要执行的命令。

| # | 修复 | 外部工具 | 本扩展执行的动作 |
| --- | --- | --- | --- |
| 01 | 剪贴板 | `Bug补丁/复制相关` | `check.sh` / `on.sh` / `off.sh` |
| 02 | 多屏幕 | `多屏幕/display-mode` | `status` / `auto` / `both` |
| 03 | 终端字体 | `终端字体` | `status` / `all 12` / `term system` |
| 04 | Root 桌面对齐 | `Root用户桌面功能对齐` | `status` / `apply` / `revert` |
| 05 | 代理一致性 | `代理工具` | `jproxy.py status` / `on.sh` / `off.sh` |

## 关键约定

- **外部工具路径由环境变量覆盖**：`GNOUS_FIXES_ROOT` 默认 `/home/root/GNOME`；工具缺失时卡片显示「未安装」，不会伪造状态。
- **体检只读、应用/回退要执行外部脚本**：所有命令走 `Command::new`，输出原样进日志面板；不把脚本逻辑搬进 Rust，避免两套实现漂移。
- **危险动作二次确认**：`on.sh --now`、`fix.sh --run --yes`、`termfont --restart`、重启 GNOME Shell 这类会关窗口的操作在本版 UI 里不直接触发，只显示命令提示。
- **状态来自真实体检**：每完成一次动作立刻重跑 `check`；列表状态、计数、日志都从 stdout 解析，不做本地缓存。

## 后续

- manifest 里声明 `[settings]`（每条修复的启用开关、默认目标值），由通用 schema 表单渲染
- 批量「全部体检」放到后台执行器，避免逐个脚本串行卡 UI
- 危险动作加统一确认弹层
