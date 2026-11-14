# GNOUS

本机 GNOME 管理面板。

## 开发宗旨

**体验好、性能好、灵活性强，不惜增加开发成本。**

这三条优先于开发成本：只要能换来更好的体验、更高的性能或更强的灵活性，多写、重写、自建一层都是可接受的；反过来，为省事而复用现成实现、把产品绑死在别人的接口约定上，不做。

所有功能取舍与架构选择都以这三条为准。遇到“复用现成实现”和“多一层自己的抽象”的取舍时，优先自己的抽象：可以薄，但必须清晰、可控。

## 技术栈

- **语言**：Rust（edition 2024）
- **UI 框架**：[GPUI](https://github.com/longbridge/gpui-kit)（`gpui-kit`，含 `gpui-base` 行为层与 `gpui-component` 样式组件层）

## 当前进度

**A · Control Deck**（`demo/ui-directions/a-cockpit.html`）已用 Rust + GPUI 复刻，概览页已接入真实系统数据：

- 主机名 / 会话 / 运行时长 / 时钟，CPU、内存、根分区、网络四项指标，每秒从 procfs·sysfs 重新采样。
- 扩展列表来自真实安装目录；点击行打开右侧详情面板（描述、版本、来源、支持的
  GNOME 版本、有无首选项），面板内按钮启用/禁用，写回 gsettings。
- 右侧快捷开关也是真数据：深色界面（`color-scheme`）、勿扰（`show-banners` 反向）、
  性能模式（`powerprofilesctl`）、夜间色温（`night-light-enabled`）；系统没有对应
  schema / 命令的开关会显示为不可用并说明原因，而不是假装有个值。

- 导航分两层：左列 rail 选分区（Overview / Extensions / Appearance / Devices / Power），
  顶栏 tab 是该分区内的页面（如 Overview → Overview / Processes）。两者共用一份
  `Nav` 状态，所以永远同步；未实现的页面显示占位而不是假数据。

- 进程页已按 `docs/EXTENSIONS.md` 做成第一个页面级扩展：`extensions/processes/manifest.toml`
  声明 5 个视图（Table / CPU / Memory / Tree / Groups），顶栏 tab 由 registry 生成，五种视图共用
  同一份 `/proc` 采样，支持点击选中与进程操作确认。五个视图现在都是真实数据。
- Table 页 P0：表头排序、直接输入搜索（名称 / PID / 用户）、全部/我的/系统范围过滤、
  暂停刷新、排序冻结、选中行详情栏（含长命令行）、表头吸顶（表体独立滚动）。
- Table 页 P1：右键菜单收拢进程操作；SIGTERM / SIGKILL / SIGSTOP / SIGCONT 都必须经过
  确认对话框（显示进程名、PID、完整命令行和信号后果，`Enter` 确认、`Esc` 取消）；
  复制整行 TSV 或命令行到剪贴板与 X11 primary selection，操作结果显示短时 toast。
  键盘：`Ctrl+1/2/3` 切范围、`Ctrl+Space` 暂停刷新、`Ctrl+F` 排序冻结、
  `Ctrl+K` / `Ctrl+Shift+K` 结束/强杀、`Ctrl+P` / `Ctrl+Shift+P` 暂停/继续进程、
  `Ctrl+C` / `Ctrl+Shift+C` 复制行/命令行、`Esc` 清空；普通字符始终进入搜索。
- Table 页 P2（Table 封版）：状态过滤、只看热点（CPU 阈值或常驻内存 ≥ 512 MB）、
  关注置顶与「只看关注」（按进程名 + 用户 + uid 持久化，避免 PID 复用）、完整键盘操作
  （`↑/↓/PgUp/PgDn/Home/End` 选行并滚动跟随、`Enter` 结束、`Delete` 强杀）、
  0.5s / 1s / 2s / 5s 刷新频率、两帧真实采样差分驱动的变化高亮、三种行密度、
  本地操作日志（信号/关注/刷新/暂停，最多 200 条，可复制 TSV）。
  新增 `Ctrl+T` 状态、`Ctrl+H` 热点、`Ctrl+W` 关注、`Ctrl+R` 刷新、`Ctrl+G` 密度、
  `Ctrl+L` 日志；偏好与日志写入 `~/.config/gnous/table.json`（临时文件 + rename 原子写）。
  Table 的选中、搜索、滚动位置不持久化，信号执行仍一律经过确认对话框。
- CPU 页（原 Live）：`/proc/stat` 聚合行差分出总占用，`cpu0..N` 差分出每核占用；
  右侧每核横条 + 各自小曲线，`ctxt` / `intr` 差分成每秒速率，`procs_running` 直读。
  总历史 120 帧，15/30/60 秒窗口按钮的标签按当前刷新率实时换算成真实时长，
  历史不足时标「采集中 n/窗口」而不画一条假线。TOP 进程每行带自己的 30 帧曲线，
  点一行就跳到 Table 并选中该行。
- Memory 页（内存地图）：方块面积 = 常驻内存，用的是全量按 RSS 排序的前 40 个进程
  （不是 CPU 前 40），其余折成「其余进程」并标明量。
  - 顶部内存条用与 `free` 一致的三分解：已用（`MemTotal - MemAvailable`）/ 可回收缓存
    （`MemAvailable - MemFree`）/ 空闲，三段**按定义精确闭合**。
    下面的「已用构成」是应用匿名 / 共享内存 / 内核 Slab / 内核栈 + 页表 / Swap 驻留 /
    **GPU 显存**（`amdgpu` 的 `mem_info_gtt_used` + `mem_info_vram_used`，核显吃系统内存
    且 `meminfo` 不报，这是最容易丢一大块的地方）/ 内核未细分——按真机实测，缺的那 2.5 GB
    正是核显占的。
  - 选中进程读 `/proc/<pid>/smaps_rollup` 得到 **PSS / USS / 共享页 / Swap PSS**（只对
    选中的那一个进程读，不做全量扫描），另带 status 里的 RssAnon / RssFile / RssShmem /
    VmSwap / VmPeak；按钮一键「在 Table 中打开」。
  - 搜索（名称 / PID）+ 全部/我的/系统范围 + 「合并同类」（同名多进程合成一块，
    Electron 类最有用）；方块用 squarified treemap，靠 `container_query` 拿真实布局尺寸，
    窗口多大就铺多大。两帧真实差分驱动 RSS 增长标记（≥ 50 MB 标 `↑`，4 秒后自动消失）。
- 新增页面级扩展 `startup-layouts`（左列独立分区 Startup）：勾选开机要铺开的桌面页面、给每页
  按比例排列窗口、配置启动时机与失败策略；配置自动写入 `~/.config/gnous/startup.json`。
  「预演一遍」真实解析每个启动命令在 `PATH` 里是否存在；执行器未接入前，「上次启动」与
  「启动日志」保持空态，不用演示数据充数。第四个视图「项目」登记自研启动项目脚本：
  `~/.config/gnous/projects/<id>/` 下的 `project.toml` + 脚本，磁盘上有什么就列什么，
  缺清单 / 缺脚本 / 缺可执行位分别标出，勾选开机直接改写项目自己的清单。
- 滚动指示器选了 `demo/scroll-indicators` 里的 **10 · LED 刻度**：右侧 26 根 2px 刻度，
  滚过的变暗红、当前位置那根拉长成亮红。整列既是指示器也是把手 —— 点哪跳哪、按住可连续拖；
  再加键盘 `↑` `↓` `PgUp` `PgDn` `Home` `End`，所以滚轮坏了也能走完整个页面。
  两个滚动区（页面外层 / 进程表体）各自独立跟踪一份 scroll handle。
- 页面级扩展按 `docs/EXTENSIONS.md` 的 manifest + registry 机制接入：
  `gnome-fixes` 把 5 个 GNOME 小补丁收成「体检 / 应用 / 回退」；其中剪贴板与 root
  桌面对齐已内建为 `src/fixes_backend/` 的 Rust 后端（结构化 facts / JSON 状态 / 审计日志），
  `root-align` 是同一后端的 C 方案诊断台，按功能域列出 8 个 systemd 用户单元；
  `displays` 接管外接屏 / 内置屏的布局：状态取自 mutter 的 `GetCurrentState`，
  APPLY 先 `METHOD_VERIFY` 校验再持久化、落地后重读确认；老脚本的
  `display-mode.timer` 只读，交棒命令只打印。
  `termfont` 统一调 GNOME 终端（pt）与 FinalShell（px）的字号：读真实当前值、
  写前备份、改完重读确认，重启 FinalShell 这种会断 SSH 的动作单独标红。
  `proxy-tools` 把本机代理中继（原本是 `/home/root/GNOME` 里的 python + bash 脚本）
  重写成自带的标准库 Rust 中继，并给出只读诊断台：旧单元只读、危险 systemd 动词
  在接口层不存在、交棒命令只打印不执行。
  另外 3 项仍走外部工具，输出不明时显示失败原因而不伪造状态；危险动作二次确认，
  `restart-shell` 不进入批量流程。
- 顶栏同时是客户端标题栏：拖动、双击最大化、右键原生窗口菜单，四边四角提供隐形缩放热区。
- Tree 页（进程树）：父子关系来自 `/proc/<pid>/stat` 的 `ppid`，从 PID 1 出发真实 DFS，
  父进程已消失的进程不会被丢掉，而是提升成自己的根（pid 复用造成 a↔b 互为父子的环也有
  专门处理，不会重复输出或挂死）。
  - 内核线程按 `stat` 里的 `PF_KTHREAD` 标志位（0x00200000）识别——它们的 `comm` 不带
    `[`，靠名字判断是错的；识别出来的只报数量、不占行。
  - 每行显示**子树合计**（CPU 与内存），所以 `firefox` 一行就是它下面所有子进程的总和，
    合计用一次后序 DFS 算出并缓存，不是每行重算。
  - 默认展开两层，`▾/▸` 逐节点折叠，另有「全部展开 / 回到两层」；折叠状态按 pid 记住，
    刷新不会自己弹开。
  - 搜索会把命中项及其所有祖先自动展开到可见；行数上限 600，超出的节点数会明确写出来，
    不静默截断。
- Groups 页（进程分组）：分组来自 `/proc/<pid>/cgroup`，按 systemd 的真实层级
  （`user.slice` → `user-1000.slice` → `user@1000.service` → `app.slice` → `xxx.scope`）
  建树聚合，不是靠字符串包含猜。
  - 这条修掉了一个真实的性能 bug：原来在渲染循环里对每个进程读一次 cgroup 文件，1 秒刷新
    就是每秒几百次文件 IO。现在采样阶段读一次，按 `(pid, starttime)` 缓存到进程生命周期
    结束，渲染只是读内存。
  - 顶部汇总卡是第一层 slice（`system.slice` / `user.slice` / …），CPU 与内存各一条占比条；
    下面是可折叠的层级行，每行带进程数、CPU 合计、内存合计和占比条。
  - `/` 是真实的根 cgroup（内核线程都在那），单独成桶并如实命名；只有**读不到** cgroup 的
    进程才计入「未归类」，两边不混。
  - 搜索（名称 / PID / cgroup 路径）+ 全部/我的/系统范围 + 按 CPU / 内存 / 进程数排序。


当前版本：`0.2.0-preview.6`。Appearance / Devices / Power 分区与启动器仍是静态占位；
剩余外部工具默认在 `/home/root/GNOME`，可用 `GNOUS_FIXES_ROOT` 覆盖；内建后端状态在
`~/.local/state/gnous/fixes/`，可用 `GNOUS_STATE_HOME` 覆盖。发布记录见 `CHANGELOG.md`。

![GNOUS 渲染](docs/gnous.png)

## 运行

```bash
export PATH="$HOME/.cargo/bin:$PATH"   # 需要 rustup 的 stable（>= 1.92）
cargo run                         # 开发版
cargo run --release              # 本机发布版
./target/release/gnous --version # 打印应用版本
```

本机安装入口为 `/usr/local/bin/gnous`，桌面入口为 `/usr/local/share/applications/gnous.desktop`。
从已构建的 release 产物安装或回滚到本机最新包：

```bash
sudo ./packaging/install-release.sh ./target/release/gnous
```

脚本会校验版本、备份被替换的旧文件、安装 `/usr/local/bin/gnous` 与 desktop/icon，
并刷新 desktop/icon 缓存；备份保存在 `/usr/local/share/gnous/release-backups/`。

## 源码结构

```
src/
├── main.rs    # 窗口、图标栏、顶栏、概览各区块的布局
├── sys.rs     # 只读系统采样（procfs / sysfs + statvfs），输出一帧 Snapshot
│              #   + 每核 CPU / ctxt-intr 速率 / 内存三分解 + GPU 显存
├── ext.rs     # 扩展清单：Provider 抽象 + 文件/gsettings 默认后端
├── registry.rs # 编译期插件注册表：解析 extensions/*/manifest.toml，生成顶栏页面
├── proc.rs    # 进程采样：/proc/<pid>/stat + status，CPU 差分、用户名、cgroup（带缓存）、
│              #   PF_KTHREAD 标志、RSS 细分与按需 smaps_rollup（PSS/USS）
├── table_state.rs # Table 视图的筛选 / 排序 / 置顶 / 变化检测 / 行密度 / 刷新频率
├── table_prefs.rs # Table 偏好与操作日志的本地持久化（~/.config/gnous/table.json）
├── scroll.rs  # LED 滚动指示器状态（刻度、拖动、键盘跳转）
├── fixes.rs   # gnome-fixes 的清单、外部命令解析与体检状态
├── fixes_backend/ # clipfix / root-align 共用的内建 Rust 后端、JSON 状态与审计
├── root_align.rs # root-align 页面状态与内建后端适配
├── proxy_tools/ # 代理工具内建后端：guard 闸门 / std 中继 / 只读诊断 / 配置
├── displays/  # 多屏幕内建后端：std 写的 D-Bus 客户端 / mutter 布局 / 配置 / 单元
├── termfont/  # 终端字体内建后端：gsettings 允许清单 / FinalShell 定点替换 / 配置
├── pages/     # 页面级扩展渲染器：processes / gnome_fixes / root_align / proxy_tools / displays / termfont
├── toggles.rs # 右侧快捷开关：gsettings bool/choice 与外部命令，缺失即置灰
├── theme.rs   # 设计 token（颜色 / 字体 / 字距 / 行高）与文本辅助函数
└── icons.rs   # 内联 SVG 线性图标（stroke 1.3，与 demo 完全一致）
```

采样很快（全是 procfs/sysfs 小文件读取），所以刷新循环直接跑在 UI 线程上，
每秒一次，不引入额外线程或锁。扩展清单每 10 秒重扫一次，避免频繁拉起 `gsettings`。

### 扩展后端

`extension-manager`（本机装了 0.6.3）是纯 GTK 前端，没有 D-Bus 服务也没有 CLI；
`org.gnome.Shell.Extensions` 又要求用户会话总线，且把数据模型绑死在 GNOME 的规则上。
所以 GNOUS 自己定了一层 `ext::Provider`：

| 后端 | 状态 | 说明 |
|---|---|---|
| `FilesystemProvider` | 默认 | 直接读 `metadata.json` + gsettings `enabled-extensions`，离线可用、无需 D-Bus |
| GNOME Shell D-Bus | 计划 | 可选，补运行时状态与扩展报错 |
| GNOUS 原生扩展 | 计划 | 通过 `GNOUS_EXTENSIONS_DIR` 的目录被扫描，不要求符合 GNOME 约定 |

`Provider` 只有三个方法（`id` / `list` / `set_enabled`），加新后端不需要动 UI。

复刻时的几个关键处理：

- 纯黑底 + 0.09/0.18 两种 alpha 的 hairline 线，模仿 Tesla 中控屏的观感。
- GPUI 没有 `letter-spacing`，用逐字 `mr()` 实现等价的字距（连最后一个字的尾距也与 CSS 一致）。
- GPUI 默认行高是 1.618（黄金比），会撑高每一行；统一改为 1.538，对齐 demo 在浏览器里的 `line-height: normal`。
- 滚动区右侧预留 12px，等价于 demo 里浏览器滚动条占用的宽度，保证各分栏边界像素对齐。
- 图标用 `Icon::data()` 直接内嵌 demo 的 SVG 路径，绕开 gpui-kit 图标集较粗的 2px 描边。

## 结构

```
demo/                     # 所有 demo，每个 demo 一个子文件夹
└── ui-directions/        # UI 方向选型（已选定 A · Control Deck）
```
