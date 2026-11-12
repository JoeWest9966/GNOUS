# GNOUS

本机 GNOME 管理面板。

## 技术栈

- **语言**：Rust（edition 2024）
- **UI 框架**：[GPUI](https://github.com/longbridge/gpui-kit)（`gpui-kit`，含 `gpui-base` 行为层与 `gpui-component` 样式组件层）

## 当前进度

**A · Control Deck**（`demo/ui-directions/a-cockpit.html`）已用 Rust + GPUI 复刻，概览页已接入真实系统数据：
主机名 / 会话 / 运行时长 / 时钟，以及 CPU、内存、根分区、网络四项指标，每秒从 procfs·sysfs 重新采样。

其余区块（扩展、开关、进程、启动器）仍是静态占位。

![GNOUS 渲染](docs/gnous.png)

## 运行

```bash
export PATH="$HOME/.cargo/bin:$PATH"   # 需要 rustup 的 stable（>= 1.92）
cargo run
```

## 源码结构

```
src/
├── main.rs    # 窗口、图标栏、顶栏、概览各区块的布局
├── sys.rs     # 只读系统采样（procfs / sysfs + statvfs），输出一帧 Snapshot
├── theme.rs   # 设计 token（颜色 / 字体 / 字距 / 行高）与文本辅助函数
└── icons.rs   # 内联 SVG 线性图标（stroke 1.3，与 demo 完全一致）
```

采样很快（全是 procfs/sysfs 小文件读取），所以刷新循环直接跑在 UI 线程上，
每秒一次，不引入额外线程或锁。

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
