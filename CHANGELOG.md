# Changelog

本文件记录 GNOUS 的可安装里程碑。版本语义：

- `0.x.y-preview.N`：开发预览版。可在本机安装、验证，但仍有占位页与未完成能力。
- `0.x.y`：功能稳定后的正式小版本。

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
