# UI 方向选型

第一轮 UI 方案 demo，共 5 个方向 + 1 个选型首页。

**已选定：A · Control Deck**（`a-cockpit.html`，纯黑中控屏风格）。

## 打开

```bash
xdg-open index.html
# 或
cd demo/ui-directions && python3 -m http.server 8080
```

## 文件

| 文件 | 方向 | 说明 |
|---|---|---|
| `index.html` | 选型首页 | 5 个方案的缩略图对比 |
| `a-cockpit.html` | A · Control Deck | 纯黑、hairline 分割、超大细体数字、Tesla 红强调 —— **已选定** |
| `b-daylight.html` | B · Daylight | 特斯拉官网风：白底大留白、巨大细体标题、双按钮 CTA |
| `c-split.html` | C · Split Cockpit | 左侧设备/环形仪表 + 右侧 Tab 内容区 |
| `d-hairline.html` | D · Hairline | 等宽字体 + 1px 网格 + 荧光绿，状态监视屏 |
| `e-bento.html` | E · Bento | 大圆角磁贴，支持明暗一键切换（`?theme=light` 可强制浅色） |

全部为纯静态 HTML，零依赖，单文件可独立打开。数据为静态假数据（ThinkPad X1 / GNOME 46.2 / Wayland 场景）。
