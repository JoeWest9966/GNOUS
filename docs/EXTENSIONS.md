# 扩展开发约定

GNOUS 的扩展与本项目**同仓开发**、共享设计语言，目标是让每个扩展都能以插件形式接入面板，而不是各写各的。

## 目录

```
extensions/
├── README.md
├── _template/                 # 新建扩展时复制
│   └── manifest.toml
└── <extension-id>/            # 一个扩展一个目录
    ├── manifest.toml          # 声明：身份、能力、配置 schema
    └── src/                   # 只有"页面级"扩展才需要 Rust 代码
```

## 两级接入

| 层级 | 适用 | 界面形态 |
| --- | --- | --- |
| **通用接入** | 大多数扩展 | 列表（搜索 / 筛选 / 批量启停）+ 选中项详情抽屉；配置表单由 schema 自动生成 |
| **页面级接入** | 配置多、逻辑重的扩展 | 在左栏 rail 注册独立页面，完全原生渲染 |

判定标准（满足任意两条，建议做成页面级）：

- 配置项 ≥ 6 项，或配置之间有联动/校验
- 有独立的数据视图（图表、列表、预览、实时状态）
- 存在长期操作流，而不只是几个开关
- 需要常驻状态或后台任务

页面级扩展同样通过 manifest 声明、由 registry 注册，**不允许在核心代码里硬编码某个扩展的名字或页面**。

## manifest 草案

```toml
id = "dash-to-dock"
name = "Dash to Dock"
summary = "可配置的应用 Dock"
version = "0.1.0"
icon = "layout-dashboard"     # 复用 gpui-kit 图标名，或自定义 svg 资源路径
capabilities = ["settings"]   # "settings" | "page"，可同时具备

# 仅页面级扩展需要：某分区下的页面入口（section 缺省为 overview）
nav = { label = "Dock", order = 30 }

# 页面级扩展还可以声明多个视图，每个视图成为顶栏的一个 tab
[[views]]
id = "table"
label = "Table"

[[views]]
id = "cpu"
label = "CPU"

# 声明式配置：界面据此自动生成表单
[settings.position]
type = "enum"
label = "停靠位置"
default = "bottom"
options = ["top", "bottom", "left", "right"]

[settings.icon_size]
type = "int"
label = "图标尺寸"
default = 48
min = 32
max = 96
unit = "px"

[settings.autohide]
type = "bool"
label = "自动隐藏"
default = true
```

支持的字段类型：`bool` / `int` / `float` / `enum` / `string` / `path` / `color` / `keybinding` / `list`。
新增类型前先在本文档登记语义，避免各扩展各写一套。

## 注册方式

- **编译期静态注册**（现行）：manifest 通过 `include_str!` 编入 `src/registry.rs` 的 `MANIFESTS`，
  页面渲染器是 `src/pages/<id>.rs` 里的普通 Rust 函数。没有 dylib、没有 ABI 问题。
- manifest 是唯一事实来源：顶栏 tab 的标签、顺序、所属分区都来自它，核心代码里不出现
  具体扩展的名字（`content()` 只按 `plugin:view` 分派）。
- 运行时列表 = 内置 registry + 系统已装的 GNOME 扩展（走现有的 `ext::Provider` 抽象），
  两者在列表页里统一呈现，来源用一个小标签区分。

### 现状

| 扩展 | 层级 | 入口 |
| --- | --- | --- |
| `processes` | 页面级 | Overview 分区下的 5 个视图：Table / CPU / Memory / Tree / Groups |
| `gnome-fixes` | 页面级 | Extensions 分区下的 Fixes：5 个小补丁统一「体检 / 应用 / 回退」；`clipfix` / `rootdesk` 由共用内建后端提供结构化状态，另外 3 项仍走外部工具 |
| `root-align` | 页面级 | Extensions 分区下的 Align：C 方案诊断台，8 个 systemd 用户单元按功能域分组；`status` / `doctor` / 动作 stdout 原样展示，健康结论来自内建 facts |
| `startup-layouts` | 页面级 | 左列独立分区 Startup，四个视图 Layouts / Boot / Journal / 项目：勾选开机铺开的页面、按比例排列窗口、配置启动策略、登记自研启动项目脚本 |
| `proxy-tools` | 页面级 | Extensions 分区下的 Proxy：诊断台三面（只读诊断 / GNOUS 自带中继 / 控制面只读展示）；`status` / `doctor` 只读，`apply` / `revert` 只写 GNOUS 自己的配置与单元文件，`handover` 只打印交棒命令，旧 `j1900-proxy.service` 只读 |
| `openshell` | 页面级 | 左列独立分区 OpenShell（立方体图标，排在 Extensions 之后）：8 个视图 / 45 个区块独占顶栏，全部读 `openshell` CLI 的真实 stdout（网关、沙箱、执行、转发、策略、凭据、日志、工具），失败原样展示；进入分区即探测一次 |

`startup-layouts` 的配置存在 `$XDG_CONFIG_HOME/gnous/startup.json`（缺省 `~/.config/gnous/startup.json`），
每次改动立即写盘；「预演一遍」只做可证明的事——把每个启动命令拿到 `PATH` 里解析，找不到就显示找不到。
真正开窗的执行器尚未实现，所以「上次启动」「启动日志」保持空态，不填演示数据。

「项目」视图管的是**用户自研的启动项目脚本**，不是 `.desktop` 也不是扩展：
一个项目 = `$XDG_CONFIG_HOME/gnous/projects/<id>/` 下的 `project.toml` 清单 + 它指向的脚本。
页面只做登记与读取——`project.toml` 里有几个就列几个，缺清单、脚本缺失、没有可执行位
都按真实情况标出来（绿 / 黄 / 红点 + 解析器原话），勾选「随开机启动」直接改写该项目的
`project.toml`，注释与其它字段原样保留。执行器未接入，所以「运行」按钮是灰的，
不会启动任何进程。

`src/fixes_backend/` 是内建补丁的共用深模块：`run(tool, op)` 返回 `Outcome { ok, output, facts }`，
状态写在 `~/.local/state/gnous/fixes/*.json`，每次动作追加 `audit.jsonl`；`GNOUS_STATE_HOME` 可覆盖（测试隔离）。
`GNOUS_FIXES_ROOT` 只用于尚未迁移的 3 个外部工具（默认 `/home/root/GNOME`）；工具缺失时显示失败原因，不伪造状态。
带副作用的动作一律二次确认，危险 flags（`--now` / `--restart` / `--run --yes`）不从 UI 触发。
`root-align` 的 `restart-shell` 会打断 GNOME 会话，放在独立危险区且二次确认；`apply / revert` 成功后自动重跑只读 `status`，并把动作 stdout 留在 `APPLY LOG`。

`src/proxy_tools/` 是代理工具的深模块：`run(op)` 返回 `Outcome { ok, output, facts }`，
UI 只用 facts 判健康。`guard.rs` 把"不许碰承载本机联网的中继"变成编译期常量与唯一
spawn 闸门（受保护单元 / 端口、禁止的 systemd 动词、`gsettings` 只能 `get`、
命令静态允许清单）；`relay.rs` 用标准库重写中继（不新增 crate，保证离线可构建）；
配置在 `~/.config/gnous/proxy-tools.json`，单元文件是 GNOUS 自己的
`gnous-proxy-relay.service`，旧单元只读。开发期约束见
`extensions/proxy-tools/DEV-SAFETY.md`。

新增页面级扩展的完整链路：`extensions/<id>/manifest.toml` → `MANIFESTS` 登记 →
`src/pages/<id>.rs` 实现 `body()` → `src/pages/mod.rs` 导出。

## 新建扩展的流程（重要）

1. 先向用户说明需求，**由助手根据功能给出接入层级、页面结构、配置项的建议**，确认后再动手；
2. 从 `extensions/_template/` 复制出 `extensions/<id>/`；
3. 在独立分支开发，验证通过后再合并、推送、删除分支（见 `agent.md`）。

## 待定

- schema 格式最终用 TOML 还是 JSON（需兼顾运行时解析与手写体验）
- 是否为扩展拆出 cargo workspace（现在是同 crate 模块；拆了能隔离编译，但要把
  `theme` / `sys` / `proc` 等基础层下沉成 lib）
- 页面级扩展的状态保持与生命周期（切页后是否保留后台任务；目前进程采样只在进程页活跃时进行）
- 与现有 extensions provider 抽象如何合并：系统扩展走 provider，自研扩展走 registry，列表层统一
