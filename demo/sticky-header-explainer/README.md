# 表头吸顶说明 demo

一句话：**表格内容向下滚动时，列名那一行始终钉在可视区顶部**；不吸顶的话滚过去就只剩一串没有标签的数字。

单文件 `index.html`，零依赖。左右两栏可**分别滚动**，对照「现状 / sticky」两种行为，底部显示滚动位置与表头是否仍在视野内。

## 打开

```bash
xdg-open index.html
# 或（本目录起一个静态服务）
cd demo/sticky-header-explainer && python3 -m http.server 8770
```

## 与 GNOUS 的对应关系

| | 现状 | 吸顶后 |
| --- | --- | --- |
| 滚动容器 | `content()` 整页滚动 | Table 自己一个内部滚动区 |
| 表头 | 普通一行，滚走 | 固定在表体顶部 |
| 排序入口 | 要滚回顶部才能点 | 任意位置可点 |
| 改造范围 | — | 只改 `pages/processes.rs` 的 `table()`，不动其它 4 个视图 |

HTML 里用 `position: sticky; top: 0` 表达效果；Rust / GPUI 里没有等价的 CSS 属性，需要把表头和表体拆成两个兄弟节点（表头固定高度、表体 `overflow_y_scroll()`），不是加一个样式就能完成。
