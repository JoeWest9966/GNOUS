# gnome-fixes · 系统修复

一个页面级扩展，把零散的 GNOME 桌面小补丁收成一个扩展。

## 设计

五项修复共用一个骨架：**体检 → 应用 → 回退**。每项都是一行卡片，点开看证据矩阵、真实结果和将要执行的命令。

| # | 修复 | 后端 | 动作 |
| --- | --- | --- | --- |
| 01 | 剪贴板 | GNOUS 内建 `clipfix` | `check` / `apply` / `revert` |
| 02 | 多屏幕 | `/home/root/GNOME/多屏幕` | `display-mode status` / `auto` / `both` |
| 03 | 终端字体 | `/home/root/GNOME/终端字体` | `termfont status` / `all 12` / `term system` |
| 04 | Root 桌面对齐 | GNOUS 内建 `root-align` | `status` / `apply` / `revert` |
| 05 | 代理一致性 | `/home/root/GNOME/代理工具` | `jproxy.py check` / `on.sh` / `off.sh` |

## 内建后端

`src/fixes_backend/` 是 `clipfix` 与 `root-align` 共用的深模块：

- 对外只有 `run(tool, op)` 与一个 `Outcome { ok, output, facts }`；`facts` 是结构化状态，UI 不再解析人类 stdout 判断健康。
- `clipfix` 保留真实 X11 / Wayland 双连接回环自测、X11 owner 探测，只改 `systemctl --user set-environment GDK_BACKEND=x11`；旧 manifest 一次性迁移为 JSON，回退按记录恢复原值。
- `root-align` 管理 8 个 root 用户的 `.d/override.conf`，扫描 `/usr/lib/systemd/user` 的条件与 journalctl 跳过记录，用 `pactl --format=json` 读音频设备；旧 `state.json` 一次性迁移，回退只删 GNOUS 拥有的覆盖。
- 状态在 `~/.local/state/gnous/fixes/`，动作为 append-only `audit.jsonl`；`GNOUS_STATE_HOME` 仅用于测试隔离。
- `/home/root/GNOME/Bug补丁/复制相关` 与 `/home/root/GNOME/Root用户桌面功能对齐` 不再是运行时依赖；迁移旧状态后可以删除整个目录。
- `restart-shell` 留在 `root-align` 页面的独立危险区；`--now` / `fix --run --yes` / `--restart` 等危险 flags 不存在于内建接口。

## 外部工具约定

- 多屏幕 / 终端字体 / 代理一致性仍由 `GNOUS_FIXES_ROOT`（默认 `/home/root/GNOME`）定位；工具缺失时卡片显示「未安装」，不会伪造状态。
- 这三项会在各自成为独立扩展时按同一后端思路迁移，本轮不改动。
- 体检只读、应用 / 回退执行真实命令；输出原样进日志面板。
- 批量「应用可用修复」只处理真实体检后 `待处理` 的项；每项动作后自动重跑体检。

## 后续

- manifest 里声明 `[settings]`（每条修复的启用开关、默认目标值），由通用 schema 表单渲染
- 危险动作加统一确认弹层
