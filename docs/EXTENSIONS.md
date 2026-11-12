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

# 仅页面级扩展需要：左栏入口
nav = { label = "Dock", order = 30 }

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

- **编译期静态注册**（推荐）：扩展是本 workspace 的 crate，在 `extensions/registry.rs` 中登记 manifest 与页面构造函数。避免 dylib ABI 问题，跨平台一致。
- 运行时列表 = 内置 registry + 系统已装的 GNOME 扩展（走现有的 provider 抽象），两者在列表页里统一呈现，来源用一个小标签区分。

## 新建扩展的流程（重要）

1. 先向用户说明需求，**由助手根据功能给出接入层级、页面结构、配置项的建议**，确认后再动手；
2. 从 `extensions/_template/` 复制出 `extensions/<id>/`；
3. 在独立分支开发，验证通过后再合并、推送、删除分支（见 `agent.md`）。

## 待定

- schema 格式最终用 TOML 还是 JSON（需兼顾运行时解析与手写体验）
- 页面级扩展的状态保持与生命周期（切页后是否保留后台任务）
- 与现有 extensions provider 抽象如何合并：系统扩展走 provider，自研扩展走 registry，列表层统一
