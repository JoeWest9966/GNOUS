# 进程页选型

第二轮 demo：进程页的 5 个信息组织方向 + 1 个选型首页。视觉语言沿用已选定的 **A · Control Deck**。

## 打开

```bash
xdg-open index.html
# 或
cd demo/process-page && python3 -m http.server 8080
```

## 文件

| 文件 | 方向 | 说明 |
|---|---|---|
| `index.html` | 选型首页 | 5 个方案缩略图对比 |
| `a-dense-table.html` | A · Dense Table | 可排序表格 + 搜索 + 全部/用户/系统过滤，行内 CPU 微条 + 趋势线 |
| `b-live-stream.html` | B · Live Stream | 30 秒 CPU 折线（每秒抖动）+ 热进程横条 + 状态分布 |
| `c-memory-map.html` | C · Memory Map | 面积 = 常驻内存的 treemap，点击方块看详情 |
| `d-process-tree.html` | D · Process Tree | 父子缩进树，可折叠，右侧命令行与子进程数 |
| `e-grouped-cards.html` | E · Grouped Cards | 按 systemd slice 分三栏，每组汇总 + top 4 |

全部为纯静态 HTML，零外部依赖，单文件可独立打开。数据为静态假数据（asus / Ryzen 5 5500U / GNOME 48 / Wayland）。

其中 A / B / C / D 带少量原生 JS：A 排序与过滤、B 每秒更新折线与数字、C 点击方块选中、D 折叠与选中。

## 选型参考

| 想回答的问题 | 推荐 |
|---|---|
| 现在谁在吃 CPU？ | A / B |
| 谁在吃内存？ | C |
| 这个进程是谁拉起来的？ | D |
| 系统资源被哪一层吃掉了？ | E |
| 日常最高频操作（结束进程、排序找热点） | A |

这五种并不互斥：A 适合作为默认页，C / D / E 可以作为顶栏 tab 里的第二层视图。最终落地形态由选型结果决定。
