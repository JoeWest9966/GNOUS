# GNOUS

本机 GNOME 管理面板。

## 技术栈

- **语言**：Rust
- **UI 框架**：[GPUI](https://github.com/longbridge/gpui-kit)（`gpui-kit`，含 `gpui-base` 行为层与 `gpui-component` 样式组件层）

## 结构

```
demo/                     # 所有 demo，每个 demo 一个子文件夹
└── ui-directions/        # UI 方向选型（已选定 A · Control Deck）
```

## 当前状态

UI 方向已确定：**A · Control Deck**（纯黑中控屏风格，Tesla 风）。

```bash
xdg-open demo/ui-directions/a-cockpit.html
```

功能与实现计划待定。
