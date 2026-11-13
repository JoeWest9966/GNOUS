# Changelog

本文件记录 GNOUS 的可安装里程碑。版本语义：

- `0.x.y-preview.N`：开发预览版。可在本机安装、验证，但仍有占位页与未完成能力。
- `0.x.y`：功能稳定后的正式小版本。

## [0.2.0-preview.5] - 2026-11-13

`clipfix` 与 `rootdesk` 两套外部脚本逻辑内嵌为 GNOUS 自带的 Rust 后端，
不再依赖 `/home/root/GNOME` 下的脚本目录。

### 内建后端 `src/fixes_backend/`

- 单一入口 `run(tool, op) -> Outcome { ok, output, facts }`：`Tool::{Clipfix, RootAlign}` ×
  `Op::{Check, Status, Doctor, Apply, Revert, RestartShell}`，UI 只看结构化 `facts`，
  不再解析人类可读 stdout（stdout 仍原样展示，失败不会被画成绿灯）。
- 一个 provider 两个入口：`gnome-fixes` 的 `rootdesk` 卡片与 `root-align` 诊断台
  共用同一份 systemd 单元逻辑，8 个单元 / 4 个功能域只有一处定义。
- 状态与审计：`~/.local/state/gnous/fixes/{clipfix,root-align}.json` +
  append-only `audit.jsonl`（只记动作与结论，不记剪贴板内容）；`GNOUS_STATE_HOME`
  可覆盖以隔离测试。
- 旧状态一次性迁移：`~/.local/state/clipfix/manifest.txt` 与
  `/root/.config/root-gnome-align/state.json` 读入新 JSON 后删除旧文件；
  迁移完成后两个外部目录可以整体删除。
- clipfix 内建真实探测：Wayland 双连接回环自测（子进程 holder + 独立 reader）、
  X11 selection owner 反查、终端后端探测；GNOUS 自身跑在 X11 时从
  `systemctl --user show-environment` 取会话 compositor，后端与探测目标解耦。
  `apply / revert` 只改 `systemctl --user set-environment GDK_BACKEND=x11`，幂等可回滚。
- root-align 内建全部动作：`status` / `doctor`（`ConditionUser` 溯源 + 日志跳过记录）/
  `apply`（写 `ConditionUser=` override 并按 socket→service 顺序启动）/ `revert`
  （只删 GNOUS 拥有的 `.d` 目录）/ `restart-shell`（失败时提示手工 `Alt+F2 r`）。
- 危险 flags（`--now` / `--restart` / `--run --yes`）在接口层就不存在；
  `restart-shell` 仍只放在 ALIGN 页独立危险区、不进批量流程。
- 范围：`多屏幕` / `终端字体` / `代理工具` 本轮不动，继续走 `GNOUS_FIXES_ROOT`
  外部工具，等各自成为独立扩展时再按同一思路迁移。

### 工程

- 新增 `src/fixes_backend/{mod,state,audit,process,clipfix,root_align,wayland,x11}.rs`；
  `src/fixes.rs` / `src/root_align.rs` 退化为适配层，只做字段映射与文案。
- `run_bounded` 改用 `poll(2)` 轮询排空管道，修掉孙进程占住管道导致
  `clipfix revert` 卡死 124 秒的问题（现在 ~0.23s 完成）。
- `cargo build` 零警告、`cargo test` 48 项通过、`cargo fmt --check` 干净。

## [0.2.0-preview.4] - 2026-11-13

Table 页封版（P2 收官），外加两个分区级调整。

### 进程 Table 封版（P2）

- **状态过滤（4）**：运行 / 睡眠 / 空闲 / IO 等待 / 停止 / 僵尸 / 其他，一键或
  `Ctrl+T` 轮换；过滤条件读 procfs 真实状态字母。
- **只看热点（5）**：CPU 阈值（1/5/10/20%，`Ctrl+H` 开关、点「阈值」轮换）或
  常驻内存 ≥ 512 MB 即算热点。
- **关注置顶（6）**：行首星标或 `Ctrl+W` 关注；按「进程名 + 用户 + uid」持久化，
  不绑定会被系统复用的 PID，重启后依然有效；关注行置顶组内仍按所选排序，
  「只看关注」一个开关只留关注行。
- **完整键盘操作（7）**：`↑/↓/PgUp/PgDn/Home/End` 在过滤后的行里移动真实选中，
  表体滚动跟随；`Enter` 结束、`Delete` 强杀（都走确认对话框）；暂停时方向键
  只滚屏不动选中。
- **刷新频率（20）**：0.5s / 1s / 2s / 5s，`Ctrl+R` 轮换；采样器按 100ms 调度，
  只有到期才读 `/proc`，离开进程页自动停采。
- **变化高亮（21）**：两帧真实采样差分驱动——新行 NEW（绿）、状态变化 STATE（橙）、
  CPU 跳变 CPU（红）、内存跳变 MEM（暗红），约 1.6 秒后自动消退；可 `Ctrl+H`
  之外的按钮整体开关。
- **行密度（30）**：紧凑 / 标准 / 宽松，`Ctrl+G` 轮换；只改行距不改字号，
  列几何不变。
- **操作日志（26）**：信号（SIGTERM / SIGKILL / SIGSTOP / SIGCONT）、关注、
  刷新率、暂停/冻结全部落日志，右侧抽屉（`Ctrl+L`）展示最近 200 条，
  时间戳到秒；可一键复制 TSV。
- **持久化**：偏好与日志写入 `$XDG_CONFIG_HOME/gnous/table.json`
  （缺省 `~/.config/gnous/table.json`），临时文件 + rename 原子写；选中、搜索、
  滚动位置不持久化。信号执行仍一律经过确认对话框。
- 新增快捷键：`Ctrl+T` 状态、`Ctrl+H` 热点、`Ctrl+W` 关注、`Ctrl+R` 刷新、
  `Ctrl+G` 密度、`Ctrl+L` 日志；普通字符仍只进搜索框。

### Startup · 项目视图

- 顶栏第四个 tab：扫描 `~/.config/gnous/projects/<id>/project.toml`，
  磁盘上有什么就列什么；状态分级（可执行 / 缺可执行位 / 脚本不存在 / 清单无法解析），
  只读脚本预览 + 复制启动命令；勾选「随开机启动」改写项目自己的 `project.toml`，
  保留注释与其它字段；执行器未接入，「运行」保持灰显。

### OpenShell 独立分区

- 左列 rail 新增立方体图标分区（排在 Extensions 之后），8 个视图独占顶栏，
  不再与 Fixes / Align 挤在同一 tab 行；未探测时指标条显示「尚未探测」，
  不把空快照算成命令失败。

### 工程

- 新增 `src/table_prefs.rs`（Table 偏好与日志持久化）、`src/table_state.rs`
  扩展筛选 / 置顶 / 变化检测 / 密度 / 刷新频率；`src/scroll.rs` 的刻度列支持
  按目标滚动区（页面 / 表体 / 日志抽屉）分派。
- `docs/EXTENSIONS.md` 现状表登记 processes / startup-layouts / openshell 的最新入口
  与配置路径。
- `cargo build` 零警告、`cargo test` 35 项通过、`cargo fmt --check` 干净。

## [0.2.0-preview.3] - 2026-11-13

新增 `openshell` 页面级扩展，把 OpenShell CLI 收成一块工作面。

### OpenShell 面板（页面级扩展 `openshell`）

- Extensions 分区新增 8 个视图 / 45 个区块：
  **Gateway**（网关状态、切换、认证身份、运行时、环境体检、指标条）、
  **Sandboxes**（总览表、创建向导、电源控制、详情、模板库、工作区、回收站）、
  **Inside**（exec 命令台、文件推拉、SSH 片段、编辑器直通、终端接入）、
  **Network**（端口转发、服务隧道、服务暴露、端点清单）、
  **Policy**（总览、增量编辑、dry-run 差异、历史、全局旋钮）、
  **Credentials**（provider、创建、附加矩阵、profile、导入校验、轮换、健康）、
  **Observe**（日志流、日志检索、设置、动作时间线、原始输出）、
  **Tooling**（命令生成器、批量预览、TUI 直通、补全、机队矩阵、危险动作审计）。
- 每个区块标注它对应的真实命令；进入视图时后台跑一次只读探测，之后可手动 `REFRESH`。
- 失败原样展示 stdout / stderr，不把失败画成绿灯；CLI 不上报的字段（网关 CPU / 内存、
  凭据到期日）就不画，不补假仪表。
- 写操作分两级：普通写动作二次确认（同一按钮点两次），删除 / 解绑 / 停转发标 `DANGER`；
  `policy update` 只走 `--dry-run`；交互式命令（`connect` / `login` / `--editor` / `term`）
  只给可复制的命令行，不在面板里代跑。
- 首版限制：GPUI 输入框未接入，创建沙箱的参数从真实数据或固定只读预设里选；
  批量动作只给命令预览，不执行。

### 工程

- `docs/EXTENSIONS.md` 现状表补 `openshell` 行；扩展自带 `extensions/openshell/README.md`。
- 选型演示保留在 `demo/openshell-panel/`。
- `cargo build` 零警告、`cargo test` 30 项通过、`cargo fmt --check` 干净。

## [0.2.0-preview.2] - 2026-11-13

新增左列独立分区的开机布局扩展，并把发布基建并入 `main`。

### 开机布局（页面级扩展 `startup-layouts`）

- 左列 rail 新增 Startup 分区，三个视图：
  - **Layouts**：页面清单（勾选 = 开机是否铺开）+ 按比例的屏幕画布 + 检查器。窗口块可点选，
    排列预设 `单窗 / 等分 / 主次 / 上下 / 网格` 按窗口数启用；新页从零开始，不继承别的页。
  - **Boot**：登录后延迟、失败重试、单页超时用 `− / +` 调；等待会话稳定、每次登录执行、
    副屏降级、补齐工作区四个开关；显示器读 `/sys/class/drm`，可重探并选择铺开屏。
  - **Journal**：执行器未接入前保持空态，不填演示时间线。
- 配置自动写 `$XDG_CONFIG_HOME/gnous/startup.json`（缺省 `~/.config/gnous/startup.json`），
  首次运行即创建，每次改动立即落盘；重启后原样读回。
- 排列只存比例不存像素；换显示器或分辨率后按比例重算，几何标签同步换算。
- 「预演一遍」把每条启动命令拿到 `PATH` 里解析，分别统计可执行 / 缺失 / 跳过；
  缺失命令标红，不假设「大概能跑」。
- 未实现且页面里已写明：真正开窗的执行器、窗口编辑（增删 / 换应用 / 改命令）、
  开机自启的 systemd unit。

### 工程

- 新增 `demo/startup-page/index.html` 作为本扩展的选型稿。
- `docs/EXTENSIONS.md` 的现状表补 `startup-layouts` 行，并登记配置路径与空态约定。
- 发布基建（`CHANGELOG.md`、`packaging/`、`gnous --version`）从 `release/v0.2.0-preview.1`
  并入 `main`，后续版本从 `main` 出。
- `cargo build` 零警告、`cargo test` 25 项通过。

### 已知范围

- Startup 的执行器未接入：勾选与排列是真实配置，但登录后不会自动开窗。
- 与 preview.1 相同的占位分区、外部工具依赖与平台范围限制。

## [0.2.0-preview.1] - 2026-11-13

第一个带版本号的预览里程碑。此前仓库没有 tag，`/usr/local/bin/gnous` 实际来自
2026-11-12 的首个 release 构建，与后续源码已经脱节；本版本把源码、桌面入口和
本机安装重新对齐。对应提交：`db47be6` 及其发布准备提交。

### 外观与窗口

- 1:1 复刻 Tesla 风格 `A · Control Deck`：纯黑、hairline、超大细体数字、Tesla 红、无圆角。
- 顶栏作为客户端标题栏：拖动、双击最大化/还原、右键 GNOME 原生窗口菜单；四边四角隐形缩放热区。
- 全局统一 `Noto Sans CJK SC`、逐字字距与 `1.538` 行高；图标使用内联 1.3px SVG。
- 新增 `10 · LED 刻度` 滚动指示器：页面外层与进程表体各自跟踪，支持点击、拖动与键盘跳转。

### 真实数据

- 概览页：CPU / 内存 / 根分区 / 网络 / 运行时长 / 主机名 / 会话 / 时钟，每秒从 procfs / sysfs 采样。
- 扩展列表：`FilesystemProvider` 读 `metadata.json` + gsettings，支持详情与启用/禁用。
- 快捷开关：深色界面、勿扰、性能模式、夜间色温；缺失 schema / 命令时显示不可用原因。
- 进程页 5 视图：Table / Live / Memory / Tree / Groups，共用 `/proc` 快照。
- Table P0/P1：排序、搜索、范围过滤、暂停、冻结、详情、吸顶、右键菜单；
  SIGTERM / SIGKILL / SIGSTOP / SIGCONT 统一二次确认；复制行 / 命令行与短时 toast。

### 扩展架构

- `docs/EXTENSIONS.md` 约定两级接入、manifest schema 与「新建扩展前先确认需求」流程。
- `src/registry.rs` 编译期读取 `extensions/*/manifest.toml`，顶栏页面从 manifest 生成；核心不硬编码扩展名。
- `extensions/processes`：`overview` 分区的 5 个页面。
- `extensions/gnome-fixes`：`extensions` 分区的 `Fixes` 页面。把 `/home/root/GNOME/` 下
  5 个小补丁收成一处，每项走「体检 → 应用 → 回退」；动作后立刻重跑体检，批量应用需二次确认。
- `extensions/root-align`：`extensions` 分区的 `Align` 页面（C 方案诊断台）。
  按 4 个功能域列出 8 个 systemd 用户单元，`STATUS` / `DOCTOR` / `APPLY LOG` 三个 tab 原样显示
  外部工具 stdout；`apply` / `revert` 成功后自动重跑只读 `status`，`restart-shell` 独立危险区二次确认。
- 外部补丁统一复用 `GNOUS_FIXES_ROOT`（默认 `/home/root/GNOME`）；工具缺失、失败或输出不明时显示
  `UNKNOWN / FAILED` 与原因，不伪造绿灯；危险 flags 与重启会话动作不从批量流程触发。

### 工程

- 分支流程写入 `agent.md`：创建独立分支开发，验证通过后才合并、推送、删除分支。
- 所有 HTML 选型 demo 按 `demo/<demo名>/` 分目录保存；本版新增 `root-align` 三方案选型 demo。
- `cargo build` 零警告、`cargo test` 22 项通过、`cargo fmt --check` 干净。
- 新增 `gnous --version` / `gnous -V`，版本值来自 `Cargo.toml` 的 `CARGO_PKG_VERSION`。
- 新增 `packaging/install-release.sh`、`packaging/gnous.desktop`、`packaging/gnous.svg`：
  校验版本、备份旧文件并原子替换本机 `/usr/local` 安装，桌面入口不再依赖手工复制。

### 已知范围

- Appearance / Devices / Power 分区与「启动器 · 常用」仍是静态占位。
- 自研扩展目前是编译期静态注册，不是运行时动态加载。
- `gnome-fixes` / `root-align` 依赖本机 `/home/root/GNOME` 下的外部脚本；换机器需设置 `GNOUS_FIXES_ROOT`。
- Windows / macOS 未验证；窗口行为主要在 GNOME 48 + X11/Xwayland / Wayland 混合会话中验收。
- `v0.2.0-preview.1` 是预览里程碑，不宣称生产稳定；正式 `0.2.0` 待占位页收尾与打包流程稳定后再发。
