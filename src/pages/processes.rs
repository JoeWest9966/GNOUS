//! The `processes` plugin: five views over one `/proc` snapshot.
//!
//! Manifest: `extensions/processes/manifest.toml` decides the tab labels and
//! order; this module decides what each one draws. Every view reads the same
//! [`crate::proc::Snapshot`], so switching tabs costs nothing.

use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    App as GpuiApp, Div, FontWeight, InteractiveElement as _, IntoElement, ParentElement as _,
    PathBuilder, Stateful, StatefulInteractiveElement as _, Styled, WeakEntity, Window, canvas,
    div, point, px, relative, rgba,
};

use crate::proc::{self, Process, Snapshot};
use crate::table_state::{Scope, SortKey};
use crate::theme::{
    accent, alpha, black, caps, dim, h, line, line_faint, line_strong, muted, ok, text, txt, v,
    warn,
};
use crate::{App, Overview};

/// Entry point from `content()`.
pub fn body(view: &str, overview: &Overview, weak: &WeakEntity<App>) -> Div {
    match view {
        "table" => table(overview, weak),
        "live" => live(overview, weak),
        "memory" => memory(overview, weak),
        "tree" => tree(overview, weak),
        "groups" => groups(overview, weak),
        other => v().child(txt(
            format!("未知视图: {other}"),
            13.,
            FontWeight::NORMAL,
            muted(),
        )),
    }
}

// ============================================================================
// shared chrome
// ============================================================================

fn page_head(overview: &Overview, title: &str, subtitle: String) -> Div {
    v().mb(px(28.))
        .child(
            caps(
                &format!("Processes · {} · 每秒刷新", overview.snap.hostname),
                11.,
                FontWeight::NORMAL,
                dim(),
                0.26,
            )
            .mb(px(14.)),
        )
        .child(txt(title, 38., FontWeight::EXTRA_LIGHT, text()).line_height(px(40.)))
        .child(txt(subtitle, 12.5, FontWeight::NORMAL, muted()).mt(px(10.)))
}

fn summary(overview: &Overview) -> Div {
    let snap = overview.proc;
    let cells = [
        (
            "进程总数",
            format!("{}", snap.processes.len()),
            "个".to_string(),
        ),
        ("运行中", format!("{}", snap.running), "running".to_string()),
        ("线程", format!("{}", snap.threads), "个".to_string()),
        ("负载", format!("{:.2}", snap.load.0), "1 分钟".to_string()),
    ];
    let mut row = h()
        .items_stretch()
        .border_t(px(1.))
        .border_b(px(1.))
        .border_color(line())
        .mb(px(28.));
    for (index, (label, value, unit)) in cells.into_iter().enumerate() {
        row = row.child(
            v().flex_1()
                .min_w_0()
                .py(px(16.))
                .when(index > 0, |this| {
                    this.pl(px(24.)).border_l(px(1.)).border_color(line())
                })
                .child(caps(label, 10., FontWeight::NORMAL, dim(), 0.22))
                .child(
                    h().items_end()
                        .gap(px(6.))
                        .mt(px(8.))
                        .child(
                            txt(value, 26., FontWeight::EXTRA_LIGHT, text()).line_height(px(28.)),
                        )
                        .child(
                            txt(unit, 10.5, FontWeight::LIGHT, muted())
                                .line_height(px(14.))
                                .mb(px(3.)),
                        ),
                ),
        );
    }
    row
}

fn state_color(state: crate::proc::State) -> gpui_kit::Hsla {
    match state.kind() {
        "running" => ok(),
        "wait" => accent(),
        "idle" => dim(),
        _ => muted(),
    }
}

fn sparkline(
    points: &[u8],
    width: f32,
    height: f32,
    color: gpui_kit::Hsla,
    hot: bool,
) -> impl IntoElement {
    let data: Vec<(f32, f32)> = if points.len() < 2 {
        vec![(0., height), (width, height)]
    } else {
        let max = points.iter().copied().max().unwrap_or(1).max(1) as f32;
        let step = width / (points.len() - 1) as f32;
        points
            .iter()
            .enumerate()
            .map(|(index, value)| {
                let y = height - (*value as f32 / max) * (height - 2.) - 1.;
                (index as f32 * step, y)
            })
            .collect()
    };
    let stroke = if hot { accent() } else { color };
    canvas(
        |_, _, _| (),
        move |bounds, _, window: &mut Window, _: &mut GpuiApp| {
            let sx = bounds.size.width.as_f32() / width.max(1.);
            let sy = bounds.size.height.as_f32() / height.max(1.);
            let mut builder = PathBuilder::stroke(px(1.));
            for (index, (x, y)) in data.iter().enumerate() {
                let p = point(bounds.origin.x + px(x * sx), bounds.origin.y + px(y * sy));
                if index == 0 {
                    builder.move_to(p);
                } else {
                    builder.line_to(p);
                }
            }
            if let Ok(path) = builder.build() {
                window.paint_path(path, stroke);
            }
        },
    )
    .w(px(width))
    .h(px(height))
}

// ============================================================================
// A · table
// ============================================================================

fn table(overview: &Overview, weak: &WeakEntity<App>) -> Div {
    let snap = overview.proc;
    let table = overview.table;
    let rows = table.rows(&snap.processes, overview.my_uid);
    let visible: Vec<&Process> = rows.into_iter().take(120).collect();
    let selected = overview
        .proc_selected
        .and_then(|pid| snap.processes.iter().find(|p| p.pid == pid));

    // Sticky header: the header + toolbar stay in a fixed slab, and only the
    // table body scrolls. GPUI has no `position: sticky`, so the two are
    // siblings rather than one long scrollable column.
    v().flex_1()
        .min_h_0()
        .child(page_head(
            overview,
            "进程",
            format!(
                "{} 个进程 · {} 个线程 · 按 {} {} · 点击行查看详情",
                snap.processes.len(),
                snap.threads,
                table.sort.label(),
                if table.descending { "降序" } else { "升序" },
            ),
        ))
        .child(summary(overview))
        .child(table_toolbar(overview, weak, visible.len()))
        .child(table_header(overview, weak))
        .child(
            v().id("proc-rows")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .children(visible.iter().enumerate().map(|(index, process)| {
                    table_row(
                        index,
                        process,
                        overview.proc_selected == Some(process.pid),
                        overview.proc_kill_armed == Some(process.pid),
                        weak,
                    )
                }))
                .child(
                    h().justify_between()
                        .py(px(18.))
                        .pb(px(28.))
                        .text_size(px(10.5))
                        .text_color(dim())
                        .child(txt(
                            format!("显示 {} / {} 个进程", visible.len(), snap.processes.len()),
                            10.5,
                            FontWeight::NORMAL,
                            dim(),
                        ))
                        .child(txt(
                            format!(
                                "CPU {:.1}% · 内存 {:.1} / {:.0} GB · 负载 {:.2}",
                                snap.cpu_total,
                                snap.mem_used_kb as f32 / 1024. / 1024.,
                                snap.mem_total_kb as f32 / 1024. / 1024.,
                                snap.load.0
                            ),
                            10.5,
                            FontWeight::NORMAL,
                            dim(),
                        )),
                ),
        )
        .when_some(selected, |this, process| {
            this.child(detail_panel(overview, Some(process), weak, "表格详情"))
        })
}

/// Search / scope / pause / freeze toolbar. Text is typed directly into the
/// search field: characters append, Backspace deletes, Esc clears.
fn table_toolbar(overview: &Overview, weak: &WeakEntity<App>, visible: usize) -> Div {
    let table = overview.table;
    let query = if table.query.is_empty() {
        "输入即可搜索名称 / PID / 用户".to_string()
    } else {
        table.query.clone()
    };
    h().items_center()
        .gap(px(10.))
        .mb(px(14.))
        .child(
            h().id("search")
                .items_center()
                .gap(px(8.))
                .w(px(280.))
                .px(px(12.))
                .py(px(9.))
                .border(px(1.))
                .border_color(line_strong())
                .child(txt(
                    query,
                    11.5,
                    FontWeight::NORMAL,
                    if table.query.is_empty() {
                        dim()
                    } else {
                        text()
                    },
                )),
        )
        .child(
            h().gap(px(1.))
                .bg(line())
                .border(px(1.))
                .border_color(line())
                .children([Scope::All, Scope::Mine, Scope::System].map(|scope| {
                    let weak = weak.clone();
                    div()
                        .id(("scope", scope as usize))
                        .cursor_pointer()
                        .px(px(13.))
                        .py(px(9.))
                        .bg(if table.scope == scope {
                            text()
                        } else {
                            black()
                        })
                        .text_color(if table.scope == scope {
                            black()
                        } else {
                            muted()
                        })
                        .on_click(move |_, _, cx| {
                            let _ = weak.update(cx, |app, cx| app.set_table_scope(scope, cx));
                        })
                        .child(caps(
                            scope.label(),
                            10.,
                            FontWeight::NORMAL,
                            if table.scope == scope {
                                black()
                            } else {
                                muted()
                            },
                            0.14,
                        ))
                })),
        )
        .child(div().flex_1())
        .child(txt(
            format!("{} 条结果", visible),
            10.5,
            FontWeight::NORMAL,
            dim(),
        ))
        .child(txt(
            if table.paused { "已暂停" } else { "实时" },
            9.5,
            FontWeight::NORMAL,
            if table.paused { warn() } else { ok() },
        ))
        .child(tool_button(
            "pause-btn",
            if table.paused {
                "继续刷新"
            } else {
                "暂停刷新"
            },
            table.paused,
            {
                let weak = weak.clone();
                Box::new(move |cx: &mut gpui_kit::App| {
                    let _ = weak.update(cx, |app, cx| app.toggle_table_pause(cx));
                }) as Box<dyn Fn(&mut gpui_kit::App)>
            },
        ))
        .child(tool_button(
            "freeze-btn",
            if table.frozen_pid.is_some() {
                "取消冻结"
            } else {
                "排序冻结"
            },
            table.frozen_pid.is_some(),
            {
                let weak = weak.clone();
                Box::new(move |cx: &mut gpui_kit::App| {
                    let _ = weak.update(cx, |app, cx| app.toggle_table_freeze(cx));
                }) as Box<dyn Fn(&mut gpui_kit::App)>
            },
        ))
}

fn tool_button(
    id: &'static str,
    label: &'static str,
    active: bool,
    handler: Box<dyn Fn(&mut gpui_kit::App)>,
) -> Stateful<Div> {
    div()
        .id(id)
        .cursor_pointer()
        .px(px(13.))
        .py(px(9.))
        .border(px(1.))
        .border_color(if active { accent() } else { line_strong() })
        .bg(if active { accent() } else { black() })
        .hover(|this| this.border_color(accent()))
        .on_click(move |_, _, cx| handler(cx))
        .child(caps(
            label,
            10.,
            FontWeight::NORMAL,
            if active { text() } else { muted() },
            0.14,
        ))
}

fn table_header(overview: &Overview, weak: &WeakEntity<App>) -> Div {
    let table = overview.table;
    let sort_cell = |label: &str, key: SortKey, width: f32| {
        let weak = weak.clone();
        let active = table.sort == key;
        let arrow = if active {
            if table.descending { " ▼" } else { " ▲" }
        } else {
            ""
        };
        let cell = div()
            .w(px(width))
            .flex_shrink_0()
            .id(("sort", key as usize))
            .cursor_pointer()
            .on_click(move |_, _, cx| {
                let _ = weak.update(cx, |app, cx| app.sort_table(key, cx));
            })
            .child(caps(
                &format!("{label}{arrow}"),
                10.,
                FontWeight::MEDIUM,
                if active { text() } else { dim() },
                0.2,
            ));
        if width > 0. {
            cell
        } else {
            cell.flex_1().min_w_0()
        }
    };
    h().items_end()
        .pb(px(12.))
        .border_b(px(1.))
        .border_color(line())
        .child(
            h().flex_1()
                .min_w_0()
                .gap(px(14.))
                .child(sort_cell("名称", SortKey::Name, 150.))
                .child(sort_cell("PID", SortKey::Pid, 80.)),
        )
        .child(sort_cell("状态", SortKey::State, 120.))
        .child(sort_cell("CPU", SortKey::Cpu, 170.))
        .child(sort_cell("内存", SortKey::Memory, 120.))
        .child(sort_cell("线程", SortKey::Threads, 80.))
        .child(
            div()
                .w(px(90.))
                .child(caps("趋势", 10., FontWeight::MEDIUM, dim(), 0.2)),
        )
        .child(div().w(px(110.)))
}

fn table_row(
    index: usize,
    process: &Process,
    selected: bool,
    armed: bool,
    weak: &WeakEntity<App>,
) -> Stateful<Div> {
    let pid = process.pid;
    let sel_weak = weak.clone();
    let kill_weak = weak.clone();
    let force_weak = weak.clone();
    let kill_pid = process.pid;
    h().items_center()
        .id(("proc", index))
        .cursor_pointer()
        .py(px(11.))
        .border_b(px(1.))
        .border_color(line_faint())
        .when(selected, |this| this.bg(alpha(0xffffff0c)))
        .hover(|this| this.bg(alpha(0xffffff06)))
        .on_click(move |_, _, cx| {
            let _ = sel_weak.update(cx, |app, cx| app.select_process(pid, cx));
        })
        .child(
            h().flex_1()
                .min_w_0()
                .items_center()
                .gap(px(8.))
                .child(txt(process.name.clone(), 12.5, FontWeight::NORMAL, text()))
                .child(txt(format!("PID {pid}"), 10.5, FontWeight::NORMAL, dim()))
                .child(txt(process.user.clone(), 10.5, FontWeight::NORMAL, dim())),
        )
        .child(div().w(px(120.)).flex_shrink_0().child(caps(
            process.state.label(),
            9.5,
            FontWeight::NORMAL,
            state_color(process.state),
            0.16,
        )))
        .child(
            h().w(px(170.))
                .flex_shrink_0()
                .items_center()
                .gap(px(10.))
                .child(
                    div().w(px(70.)).h(px(1.)).bg(line_strong()).child(
                        div()
                            .h(px(3.))
                            .w(relative(((process.cpu / 100.) * 4.).clamp(0., 1.)))
                            .bg(if process.cpu > 20. { accent() } else { text() }),
                    ),
                )
                .child(txt(
                    format!("{:.1}%", process.cpu),
                    11.5,
                    FontWeight::NORMAL,
                    muted(),
                )),
        )
        .child(div().w(px(120.)).flex_shrink_0().child(txt(
            process.mem_label(),
            11.5,
            FontWeight::NORMAL,
            muted(),
        )))
        .child(div().w(px(80.)).flex_shrink_0().child(txt(
            format!("{}", process.threads),
            11.5,
            FontWeight::NORMAL,
            muted(),
        )))
        .child(div().w(px(90.)).flex_shrink_0().child(sparkline(
            &process.history,
            64.,
            14.,
            rgba(0xffffffb3).into(),
            process.cpu > 20.,
        )))
        .child(
            h().w(px(110.))
                .flex_shrink_0()
                .gap(px(4.))
                .justify_end()
                .child(
                    div()
                        .id(("kill", index))
                        .cursor_pointer()
                        .px(px(8.))
                        .py(px(4.))
                        .border(px(1.))
                        .border_color(if armed { accent() } else { line_strong() })
                        .text_color(if armed { accent() } else { muted() })
                        .hover(|this| this.border_color(accent()).text_color(accent()))
                        .on_click(move |_, _, cx| {
                            let _ =
                                kill_weak.update(cx, |app, cx| app.arm_kill(kill_pid, false, cx));
                        })
                        .child(caps(
                            if armed { "确认" } else { "结束" },
                            9.,
                            FontWeight::NORMAL,
                            if armed { accent() } else { muted() },
                            0.12,
                        )),
                )
                .child(
                    div()
                        .id(("force", index))
                        .cursor_pointer()
                        .px(px(7.))
                        .py(px(4.))
                        .border(px(1.))
                        .border_color(if armed { accent() } else { line_strong() })
                        .hover(|this| this.border_color(accent()).text_color(accent()))
                        .on_click(move |_, _, cx| {
                            let _ =
                                force_weak.update(cx, |app, cx| app.arm_kill(kill_pid, true, cx));
                        })
                        .child(caps("强杀", 9., FontWeight::NORMAL, dim(), 0.12)),
                ),
        )
}

// ============================================================================
// B · live
// ============================================================================

fn live(overview: &Overview, weak: &WeakEntity<App>) -> Div {
    let snap = overview.proc;
    let max = snap.processes.iter().map(|p| p.cpu).fold(1., f32::max);
    let top: Vec<&Process> = snap.processes.iter().take(8).collect();
    let history = overview.cpu_history;
    let spark: Vec<u8> = history.iter().map(|v| v.clamp(0., 100.) as u8).collect();

    h().items_stretch()
        .gap(px(1.))
        .bg(line())
        .border(px(1.))
        .border_color(line())
        .child(
            v().flex_1()
                .min_w_0()
                .bg(black())
                .p(px(28.))
                .child(
                    h().items_end()
                        .justify_between()
                        .child(
                            v().child(caps("CPU 总占用", 10., FontWeight::NORMAL, dim(), 0.22))
                                .child(
                                    h().items_end()
                                        .gap(px(6.))
                                        .mt(px(8.))
                                        .child(
                                            txt(
                                                format!("{:.1}", snap.cpu_total),
                                                44.,
                                                FontWeight::EXTRA_LIGHT,
                                                text(),
                                            )
                                            .line_height(px(46.)),
                                        )
                                        .child(
                                            txt("%", 14., FontWeight::LIGHT, muted()).mb(px(6.)),
                                        ),
                                ),
                        )
                        .child(
                            v().items_end()
                                .child(caps("最近 30 秒", 9.5, FontWeight::NORMAL, dim(), 0.2))
                                .child(
                                    txt(
                                        format!(
                                            "峰值 {:.0}%",
                                            history.iter().fold(0f32, |m, v| m.max(*v))
                                        ),
                                        10.5,
                                        FontWeight::NORMAL,
                                        muted(),
                                    )
                                    .mt(px(6.)),
                                ),
                        ),
                )
                .child(div().h(px(150.)).w_full().mt(px(18.)).child(sparkline(
                    &spark,
                    720.,
                    150.,
                    rgba(0xffffffcc).into(),
                    false,
                )))
                .child(div().h(px(1.)).w_full().bg(line()).my(px(20.)))
                .child(caps("TOP 进程", 10., FontWeight::NORMAL, dim(), 0.22).mb(px(10.)))
                .children(top.iter().map(|process| {
                    let width = (process.cpu / max).clamp(0., 1.);
                    let pid = process.pid;
                    let sel = overview.proc_selected == Some(pid);
                    let weak = weak.clone();
                    v().id(("hot", pid))
                        .cursor_pointer()
                        .py(px(7.))
                        .when(sel, |this| this.bg(alpha(0xffffff0a)))
                        .on_click(move |_, _, cx| {
                            let _ = weak.update(cx, |app, cx| app.select_process(pid, cx));
                        })
                        .child(
                            h().items_center()
                                .gap(px(14.))
                                .child(
                                    v().w(px(150.))
                                        .min_w_0()
                                        .child(txt(
                                            process.name.clone(),
                                            12.,
                                            FontWeight::NORMAL,
                                            text(),
                                        ))
                                        .child(txt(
                                            format!("PID {pid}"),
                                            9.5,
                                            FontWeight::NORMAL,
                                            dim(),
                                        )),
                                )
                                .child(div().flex_1().h(px(1.)).bg(line_strong()).child(
                                    div().h(px(3.)).w(relative(width)).bg(if process.cpu > 20. {
                                        accent()
                                    } else {
                                        text()
                                    }),
                                ))
                                .child(div().w(px(56.)).flex().justify_end().child(txt(
                                    format!("{:.1}%", process.cpu),
                                    11.5,
                                    FontWeight::NORMAL,
                                    muted(),
                                ))),
                        )
                })),
        )
        .child(
            v().w(px(300.))
                .flex_shrink_0()
                .bg(black())
                .p(px(26.))
                .child(caps("状态分布", 10., FontWeight::NORMAL, dim(), 0.22))
                .child(state_stack(snap))
                .child(div().h(px(1.)).w_full().bg(line()).my(px(22.)))
                .child(caps("系统", 10., FontWeight::NORMAL, dim(), 0.22))
                .child(stat_line(
                    "内存",
                    format!(
                        "{:.1} / {:.0} GB",
                        snap.mem_used_kb as f32 / 1024. / 1024.,
                        snap.mem_total_kb as f32 / 1024. / 1024.
                    ),
                ))
                .child(stat_line("线程", format!("{}", snap.threads)))
                .child(stat_line(
                    "负载",
                    format!("{:.2} / {:.2}", snap.load.0, snap.load.1),
                ))
                .child(stat_line("运行时长", overview.snap.detail.clone())),
        )
}

fn state_stack(snap: &Snapshot) -> Div {
    let total = snap.processes.len().max(1) as f32;
    let count = |kind: &str| {
        snap.processes
            .iter()
            .filter(|p| p.state.kind() == kind)
            .count()
    };
    let running = count("running");
    let wait = count("wait");
    let idle = count("idle");
    let sleep = snap.processes.len().saturating_sub(running + wait + idle);
    v().mt(px(14.))
        .child(
            h().h(px(6.))
                .gap(px(2.))
                .child(bar_segment(running as f32 / total, ok()))
                .child(bar_segment(sleep as f32 / total, muted()))
                .child(bar_segment(wait as f32 / total, accent()))
                .child(bar_segment(idle as f32 / total, dim())),
        )
        .child(
            v().mt(px(12.))
                .child(state_line("运行中", running, ok()))
                .child(state_line("休眠", sleep, muted()))
                .child(state_line("IO 等待", wait, accent()))
                .child(state_line("空闲", idle, dim())),
        )
}

fn bar_segment(width: f32, color: gpui_kit::Hsla) -> Div {
    div().h_full().w(relative(width)).bg(color)
}

fn state_line(label: &str, count: usize, color: gpui_kit::Hsla) -> Div {
    h().items_center()
        .gap(px(8.))
        .py(px(3.))
        .child(div().size(px(5.)).rounded_full().bg(color))
        .child(txt(label, 11., FontWeight::NORMAL, muted()))
        .child(div().flex_1())
        .child(txt(format!("{count}"), 11., FontWeight::NORMAL, text()))
}

fn stat_line(label: &str, value: String) -> Div {
    h().justify_between()
        .py(px(7.))
        .child(txt(label.to_string(), 11.5, FontWeight::NORMAL, muted()))
        .child(txt(value, 11.5, FontWeight::NORMAL, text()))
}

// ============================================================================
// C · memory map
// ============================================================================

struct Block {
    pid: u32,
    name: String,
    rss_kb: u64,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
}

fn memory(overview: &Overview, weak: &WeakEntity<App>) -> Div {
    let snap = overview.proc;
    let mut procs: Vec<&Process> = snap.processes.iter().take(60).collect();
    procs.sort_by(|a, b| b.rss_kb.cmp(&a.rss_kb));
    let mut items: Vec<(u32, String, u64)> = procs
        .iter()
        .map(|p| (p.pid, p.name.clone(), p.rss_kb))
        .collect();
    let total: u64 = items.iter().map(|item| item.2).sum();
    let used_total = snap.mem_used_kb.max(total);
    items.push((0, "其余".into(), used_total.saturating_sub(total)));

    let blocks = tile(&items, 0., 0., 780., 420.);
    let selected = overview
        .proc_selected
        .and_then(|pid| snap.processes.iter().find(|p| p.pid == pid))
        .or_else(|| snap.processes.first());

    h().items_stretch()
        .gap(px(1.))
        .bg(line())
        .border(px(1.))
        .border_color(line())
        .child(
            v().flex_1()
                .min_w_0()
                .bg(black())
                .p(px(22.))
                .child(
                    h().items_end()
                        .justify_between()
                        .mb(px(6.))
                        .child(
                            v().child(caps("内存地图", 10., FontWeight::NORMAL, dim(), 0.22))
                                .child(
                                    txt("面积 = 常驻内存", 11.5, FontWeight::NORMAL, muted())
                                        .mt(px(6.)),
                                ),
                        )
                        .child(txt(
                            format!(
                                "{:.1} / {:.0} GB",
                                snap.mem_used_kb as f32 / 1024. / 1024.,
                                snap.mem_total_kb as f32 / 1024. / 1024.
                            ),
                            22.,
                            FontWeight::EXTRA_LIGHT,
                            text(),
                        )),
                )
                .child(
                    div()
                        .relative()
                        .w_full()
                        .h(px(420.))
                        .children(blocks.into_iter().map(|block| {
                            let pid = block.pid;
                            let sel = overview.proc_selected == Some(pid);
                            let weak = weak.clone();
                            let shade = if block.pid == 0 {
                                alpha(0xffffff06)
                            } else {
                                alpha(
                                    0xffffff0e
                                        + (block.rss_kb.min(2_000_000) / 200_000) as u32
                                            * 0x00000008,
                                )
                            };
                            let label = if block.w > 90. && block.h > 30. {
                                Some((block.name.clone(), block.rss_kb))
                            } else {
                                None
                            };
                            v().absolute()
                                .id(("blk", block.pid as usize))
                                .cursor_pointer()
                                .left(px(block.x))
                                .top(px(block.y))
                                .w(px((block.w - 2.).max(1.)))
                                .h(px((block.h - 2.).max(1.)))
                                .bg(if sel { alpha(0xe3193728) } else { shade })
                                .border(px(1.))
                                .border_color(if sel { accent() } else { alpha(0xffffff10) })
                                .p(px(7.))
                                .when(pid > 0, |this| {
                                    this.on_click(move |_, _, cx| {
                                        let _ =
                                            weak.update(cx, |app, cx| app.select_process(pid, cx));
                                    })
                                })
                                .when_some(label, |this, (name, rss)| {
                                    this.child(txt(name, 11., FontWeight::NORMAL, text()))
                                        .child(txt(
                                            format!("{:.0} MB", rss as f32 / 1024.),
                                            9.5,
                                            FontWeight::NORMAL,
                                            dim(),
                                        ))
                                })
                        })),
                ),
        )
        .child(detail_panel(overview, selected, weak, "选中进程"))
}

/// Slice-and-dice treemap; alternating direction keeps blocks readable.
fn tile(items: &[(u32, String, u64)], x: f32, y: f32, w: f32, h: f32) -> Vec<Block> {
    if items.is_empty() {
        return Vec::new();
    }
    if items.len() == 1 {
        let (pid, name, rss) = items[0].clone();
        return vec![Block {
            pid,
            name,
            rss_kb: rss,
            x,
            y,
            w,
            h,
        }];
    }
    let total: u64 = items.iter().map(|item| item.2).sum::<u64>().max(1);
    let mut acc = 0u64;
    let mut split = items.len();
    for (index, item) in items.iter().enumerate() {
        acc += item.2;
        if acc as f32 / total as f32 >= 0.5 {
            split = index + 1;
            break;
        }
    }
    let (head, tail) = items.split_at(split.min(items.len() - 1));
    let head_sum: u64 = head.iter().map(|item| item.2).sum();
    let frac = head_sum as f32 / total as f32;
    if w >= h {
        let head_w = w * frac;
        let mut out = tile(head, x, y, head_w, h);
        out.extend(tile(tail, x + head_w, y, w - head_w, h));
        out
    } else {
        let head_h = h * frac;
        let mut out = tile(head, x, y, w, head_h);
        out.extend(tile(tail, x, y + head_h, w, h - head_h));
        out
    }
}

fn detail_panel(
    overview: &Overview,
    process: Option<&Process>,
    weak: &WeakEntity<App>,
    title: &str,
) -> Div {
    let Some(process) = process else {
        return v().w(px(300.)).bg(black()).p(px(24.));
    };
    let pid = process.pid;
    let armed = overview.proc_kill_armed == Some(pid);
    let kill_weak = weak.clone();
    let force_weak = weak.clone();
    v().w(px(300.))
        .flex_shrink_0()
        .bg(black())
        .p(px(24.))
        .child(caps(title, 10., FontWeight::NORMAL, dim(), 0.22))
        .child(
            txt(process.name.clone(), 19., FontWeight::LIGHT, text())
                .line_height(px(22.))
                .mt(px(12.)),
        )
        .child(
            txt(
                format!("PID {pid} · {}", process.user),
                10.5,
                FontWeight::NORMAL,
                dim(),
            )
            .mt(px(5.)),
        )
        .child(
            h().items_end()
                .gap(px(5.))
                .mt(px(20.))
                .child(
                    txt(
                        format!("{:.1}", process.rss_mb() / 1024.),
                        34.,
                        FontWeight::EXTRA_LIGHT,
                        text(),
                    )
                    .line_height(px(36.)),
                )
                .child(txt("GB 常驻", 10.5, FontWeight::LIGHT, muted()).mb(px(4.))),
        )
        .child(
            v().mt(px(16.))
                .border_t(px(1.))
                .border_color(line())
                .child(stat_line("CPU", format!("{:.1}%", process.cpu)))
                .child(stat_line("线程", format!("{}", process.threads)))
                .child(stat_line("状态", process.state.label().to_string()))
                .child(stat_line("启动于", process.age_label()))
                .child(stat_line("PPID", format!("{}", process.ppid))),
        )
        .child(
            v().mt(px(16.))
                .child(caps("命令行", 9.5, FontWeight::NORMAL, dim(), 0.2))
                .child(
                    txt(
                        {
                            let cmd = proc::command(pid);
                            if cmd.is_empty() {
                                "—（内核线程）".to_string()
                            } else {
                                cmd
                            }
                        },
                        10.5,
                        FontWeight::NORMAL,
                        muted(),
                    )
                    .line_height(px(15.))
                    .mt(px(6.)),
                ),
        )
        .child(
            h().gap(px(8.))
                .mt(px(20.))
                .child(
                    div()
                        .id(("detail-kill", pid as usize))
                        .flex_1()
                        .cursor_pointer()
                        .justify_center()
                        .py(px(10.))
                        .border(px(1.))
                        .border_color(if armed { accent() } else { line_strong() })
                        .bg(if armed { accent() } else { alpha(0x00000000) })
                        .on_click(move |_, _, cx| {
                            let _ = kill_weak.update(cx, |app, cx| app.arm_kill(pid, false, cx));
                        })
                        .child(caps(
                            if armed {
                                "再点一次确认"
                            } else {
                                "结束进程"
                            },
                            9.5,
                            FontWeight::NORMAL,
                            if armed { text() } else { muted() },
                            0.14,
                        )),
                )
                .child(
                    div()
                        .id(("detail-force", pid as usize))
                        .cursor_pointer()
                        .justify_center()
                        .px(px(12.))
                        .py(px(10.))
                        .border(px(1.))
                        .border_color(line_strong())
                        .hover(|this| this.border_color(accent()).text_color(accent()))
                        .on_click(move |_, _, cx| {
                            let _ = force_weak.update(cx, |app, cx| app.arm_kill(pid, true, cx));
                        })
                        .child(caps("强杀", 9.5, FontWeight::NORMAL, dim(), 0.14)),
                ),
        )
}

// ============================================================================
// D · tree
// ============================================================================

fn tree(overview: &Overview, weak: &WeakEntity<App>) -> Div {
    let snap = overview.proc;
    let mut children: std::collections::HashMap<u32, Vec<&Process>> =
        std::collections::HashMap::new();
    for process in &snap.processes {
        children.entry(process.ppid).or_default().push(process);
    }
    let roots: Vec<&Process> = snap.processes.iter().filter(|p| p.pid == 1).collect();
    let orphans: Vec<&Process> = snap
        .processes
        .iter()
        .filter(|p| p.ppid == 0 || p.ppid == 1 || !children.contains_key(&p.ppid))
        .filter(|p| p.pid != 1)
        .collect();

    let mut rows: Vec<Stateful<Div>> = Vec::new();
    for root in roots {
        push_tree(&mut rows, root, 0, overview, weak, &children);
    }
    for root in orphans {
        push_tree(&mut rows, root, 0, overview, weak, &children);
    }

    v().child(page_head(
        overview,
        "进程树",
        format!(
            "{} 个进程 · 按父子关系展开 · 点击行查看详情",
            snap.processes.len()
        ),
    ))
    .children(rows)
    .child(
        txt(
            "树来自 /proc/<pid>/stat 的 ppid；只为 CPU 最高的 300 个进程建树。",
            10.,
            FontWeight::NORMAL,
            dim(),
        )
        .mt(px(18.)),
    )
}

fn push_tree(
    rows: &mut Vec<Stateful<Div>>,
    process: &Process,
    depth: usize,
    overview: &Overview,
    weak: &WeakEntity<App>,
    children: &std::collections::HashMap<u32, Vec<&Process>>,
) {
    if depth > 8 || rows.len() > 300 {
        return;
    }
    let pid = process.pid;
    let selected = overview.proc_selected == Some(pid);
    let row_weak = weak.clone();
    let padding = 8. + depth as f32 * 18.;
    rows.push(
        h().id(("tree", pid as usize))
            .relative()
            .cursor_pointer()
            .items_center()
            .gap(px(10.))
            .pl(px(padding))
            .py(px(8.))
            .border_b(px(1.))
            .border_color(line_faint())
            .when(selected, |this| {
                this.bg(alpha(0xffffff0c)).child(
                    div()
                        .absolute()
                        .left(px(0.))
                        .top(px(6.))
                        .bottom(px(6.))
                        .w(px(2.))
                        .bg(accent()),
                )
            })
            .hover(|this| this.bg(alpha(0xffffff06)))
            .on_click(move |_, _, cx| {
                let _ = row_weak.update(cx, |app, cx| app.select_process(pid, cx));
            })
            .child(div().w(px(10.)).child(txt(
                if depth > 0 { "└" } else { "" },
                10.,
                FontWeight::NORMAL,
                dim(),
            )))
            .child(txt(process.name.clone(), 12.5, FontWeight::NORMAL, text()))
            .child(txt(format!("{pid}"), 10., FontWeight::NORMAL, dim()))
            .child(
                div()
                    .size(px(5.))
                    .rounded_full()
                    .bg(state_color(process.state)),
            )
            .child(div().flex_1())
            .child(txt(
                format!("CPU {:.1}%", process.cpu),
                10.5,
                FontWeight::NORMAL,
                dim(),
            ))
            .child(txt(process.mem_label(), 10.5, FontWeight::NORMAL, dim()))
            .child(div().w(px(90.)).flex().justify_end().child(txt(
                format!("{} 线程", process.threads),
                10.5,
                FontWeight::NORMAL,
                dim(),
            ))),
    );
    if let Some(kids) = children.get(&pid) {
        for child in kids {
            push_tree(rows, child, depth + 1, overview, &weak, children);
        }
    }
}

// ============================================================================
// E · groups
// ============================================================================

fn groups(overview: &Overview, weak: &WeakEntity<App>) -> Div {
    let snap = overview.proc;
    let mut system: Vec<&Process> = Vec::new();
    let mut session: Vec<&Process> = Vec::new();
    let mut apps: Vec<&Process> = Vec::new();
    let mut other: Vec<&Process> = Vec::new();
    for process in &snap.processes {
        let slices = proc::cgroups(process.pid);
        let joined = slices.join("/");
        if joined.contains("app.slice") || joined.contains("apps.slice") {
            apps.push(process);
        } else if joined.contains("user@") || joined.contains("user.slice") {
            session.push(process);
        } else if joined.starts_with("system.slice") || joined.contains("system.slice") {
            system.push(process);
        } else {
            other.push(process);
        }
    }
    let group = |title: &str, list: Vec<&Process>, tint: gpui_kit::Hsla| {
        group_card(title, &list, tint, weak)
    };
    h().items_stretch()
        .gap(px(1.))
        .bg(line())
        .border(px(1.))
        .border_color(line())
        .child(group("系统服务 · system.slice", system, text()))
        .child(group("用户会话 · user.slice", session, muted()))
        .child(group("前台应用 · app.slice", apps, accent()))
        .when(!other.is_empty(), |this| {
            this.child(group("其他 · 未归类", other, dim()))
        })
}

fn group_card(
    title: &str,
    processes: &[&Process],
    tint: gpui_kit::Hsla,
    weak: &WeakEntity<App>,
) -> Div {
    let cpu: f32 = processes.iter().map(|p| p.cpu).sum();
    let mem: u64 = processes.iter().map(|p| p.rss_kb).sum();
    let max = processes.iter().map(|p| p.cpu).fold(1., f32::max);
    let mut list = processes.to_vec();
    list.sort_by(|a, b| b.cpu.total_cmp(&a.cpu));
    let top: Vec<&Process> = list.iter().take(5).copied().collect();
    let mut rows: Vec<Stateful<Div>> = Vec::new();
    for process in &top {
        let pid = process.pid;
        let weak = weak.clone();
        rows.push(
            h().id(("grp", pid as usize))
                .cursor_pointer()
                .justify_between()
                .items_end()
                .py(px(8.))
                .border_b(px(1.))
                .border_color(line_faint())
                .hover(|this| this.bg(alpha(0xffffff06)))
                .on_click(move |_, _, cx| {
                    let _ = weak.update(cx, |app, cx| app.select_process(pid, cx));
                })
                .child(
                    v().min_w_0()
                        .child(txt(process.name.clone(), 11.5, FontWeight::NORMAL, text()))
                        .child(txt(
                            format!("{} · {}", process.user, process.state.label()),
                            9.5,
                            FontWeight::NORMAL,
                            dim(),
                        )),
                )
                .child(
                    v().items_end()
                        .child(txt(
                            format!("{:.1}%", process.cpu),
                            11.,
                            FontWeight::NORMAL,
                            muted(),
                        ))
                        .child(txt(process.mem_label(), 9.5, FontWeight::NORMAL, dim())),
                ),
        );
    }
    v().flex_1()
        .min_w_0()
        .bg(black())
        .p(px(22.))
        .child(caps(title, 10., FontWeight::NORMAL, dim(), 0.22))
        .child(
            h().items_end()
                .gap(px(6.))
                .mt(px(12.))
                .child(
                    txt(format!("{:.1}", cpu), 28., FontWeight::EXTRA_LIGHT, text())
                        .line_height(px(30.)),
                )
                .child(txt("% CPU", 10., FontWeight::LIGHT, muted()).mb(px(4.))),
        )
        .child(
            txt(
                format!(
                    "{} 个进程 · 内存 {:.1} GB",
                    processes.len(),
                    mem as f32 / 1024. / 1024.
                ),
                10.,
                FontWeight::NORMAL,
                dim(),
            )
            .mt(px(4.)),
        )
        .child(
            h().h(px(3.))
                .w_full()
                .mt(px(14.))
                .gap(px(1.))
                .children(top.iter().map(|process| {
                    div()
                        .h_full()
                        .w(relative((process.cpu / max).clamp(0., 1.)))
                        .bg(tint)
                        .opacity(0.4 + (process.cpu / max) * 0.6)
                })),
        )
        .child(v().mt(px(16.)).children(rows))
}
