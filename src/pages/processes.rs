//! The `processes` plugin: five views over one `/proc` snapshot.
//!
//! Manifest: `extensions/processes/manifest.toml` decides the tab labels and
//! order; this module decides what each one draws. Every view reads the same
//! [`crate::proc::Snapshot`], so switching tabs costs nothing.

use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    App as GpuiApp, Context, Div, FontWeight, InteractiveElement as _, IntoElement, MouseButton,
    ParentElement as _, PathBuilder, Stateful, StatefulInteractiveElement as _, Styled, WeakEntity,
    Window, canvas, div, point, px, relative, rgba,
};

use crate::proc::{self, Process, Snapshot};
use crate::table_state::{ChangeKind, ROW_LIMIT, Scope, SortKey, StateFilter};
use crate::theme::{
    accent, accent_hover, alpha, black, caps, dim, h, line, line_faint, line_strong, muted, ok,
    surface_2, text, txt, v, warn,
};
use crate::{App, Overview, ProcAction, ProcOverlays};

/// Entry point from `content()`.
pub fn body(view: &str, overview: &Overview, weak: &WeakEntity<App>) -> Div {
    match view {
        "table" => table(overview, weak),
        "cpu" => cpu(overview, weak),
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
                &format!(
                    "Processes · {} · {} 刷新",
                    overview.snap.hostname,
                    overview.table.refresh.label()
                ),
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
    let visible: Vec<&Process> = rows.into_iter().take(ROW_LIMIT).collect();
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
                "{} 个进程 · {} 个线程 · 按 {} {} · ↑↓ 选行 / Enter 结束 / Ctrl+W 关注 / 右键更多",
                snap.processes.len(),
                snap.threads,
                table.sort.label(),
                if table.descending { "降序" } else { "升序" },
            ),
        ))
        .child(summary(overview))
        .child(table_toolbar(overview, weak, visible.len()))
        .child(table_header(overview, weak).pr(px(26.)))
        .child(
            v().relative()
                .flex_1()
                .min_h_0()
                .child(
                    v().id("proc-rows")
                        .flex_1()
                        .min_h_0()
                        .pr(px(26.))
                        .overflow_y_scroll()
                        .track_scroll(overview.table_led.handle())
                        .children(
                            visible
                                .iter()
                                .enumerate()
                                .map(|(index, process)| table_row(index, process, overview, weak)),
                        )
                        .child(
                            h().justify_between()
                                .py(px(18.))
                                .pb(px(28.))
                                .text_size(px(10.5))
                                .text_color(dim())
                                .child(txt(
                                    format!(
                                        "显示 {} / {} 个进程",
                                        visible.len(),
                                        snap.processes.len()
                                    ),
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
                .child(crate::led_column(
                    "led-table",
                    overview.table_led,
                    4.,
                    crate::scroll::Target::Table,
                    weak,
                )),
        )
        .when_some(selected, |this, process| {
            this.child(detail_panel(
                overview,
                Some(process),
                weak,
                "表格详情",
                true,
            ))
        })
}

/// Search / scope / pause / freeze toolbar. Text is typed directly into the
/// search field: characters append, Backspace deletes, Esc clears.
fn table_toolbar(overview: &Overview, weak: &WeakEntity<App>, visible: usize) -> Div {
    let table = overview.table;
    let query = if table.query.is_empty() {
        "输入搜索 · ↑↓ 选择 · Ctrl+动作".to_string()
    } else {
        table.query.clone()
    };
    let mut controls = v().gap(px(9.)).mb(px(14.));
    let mut top = h().items_center().gap(px(9.)).child(
        h().id("search")
            .items_center()
            .gap(px(8.))
            .w(px(252.))
            .px(px(12.))
            .py(px(8.))
            .border(px(1.))
            .border_color(line_strong())
            .child(txt(
                query,
                11.,
                FontWeight::NORMAL,
                if table.query.is_empty() {
                    dim()
                } else {
                    text()
                },
            )),
    );
    for scope in [Scope::All, Scope::Mine, Scope::System] {
        top = top.child(scope_button(scope, table.scope == scope, weak));
    }
    for filter in StateFilter::ALL {
        top = top.child(state_button(filter, table.state_filter == filter, weak));
    }
    top = top
        .child(div().flex_1())
        .child(txt(
            format!("{visible} 条"),
            10.5,
            FontWeight::NORMAL,
            dim(),
        ))
        .child(txt(
            if table.paused { "已暂停" } else { "实时" },
            9.5,
            FontWeight::NORMAL,
            if table.paused { warn() } else { ok() },
        ));
    controls = controls.child(top);

    let mut bottom = h()
        .items_center()
        .flex_wrap()
        .gap(px(7.))
        .child(click_tool(
            "hot-btn",
            &format!("只看热点 · CPU ≥ {:.0}%", table.hot_threshold()),
            table.hot_only,
            App::toggle_table_hot,
            weak,
        ))
        .child(click_tool(
            "hot-threshold",
            "阈值",
            false,
            App::cycle_table_hot,
            weak,
        ))
        .child(click_tool(
            "watch-only",
            &format!("只看关注 · {}", table.watch_count()),
            table.watched_only,
            App::toggle_table_watched_only,
            weak,
        ))
        .child(click_tool(
            "refresh-btn",
            &format!("刷新 · {}", table.refresh.label()),
            false,
            App::cycle_table_refresh,
            weak,
        ))
        .child(click_tool(
            "density-btn",
            &format!("密度 · {}", table.density.label()),
            false,
            App::cycle_table_density,
            weak,
        ))
        .child(click_tool(
            "highlight-btn",
            if table.highlights_on {
                "变化高亮 · 开"
            } else {
                "变化高亮 · 关"
            },
            table.highlights_on,
            App::toggle_table_highlights,
            weak,
        ))
        .child(click_tool(
            "log-btn",
            "操作日志 · Ctrl+L",
            false,
            App::toggle_proc_log,
            weak,
        ))
        .child(div().flex_1())
        .child(click_tool(
            "pause-btn",
            if table.paused {
                "继续刷新"
            } else {
                "暂停刷新"
            },
            table.paused,
            App::toggle_table_pause,
            weak,
        ))
        .child(click_tool(
            "freeze-btn",
            if table.frozen_pid.is_some() {
                "取消冻结"
            } else {
                "排序冻结"
            },
            table.frozen_pid.is_some(),
            App::toggle_table_freeze,
            weak,
        ));
    if let Some(error) = overview.table_error {
        bottom = bottom.child(txt(error.to_string(), 9.5, FontWeight::NORMAL, warn()));
    }
    controls = controls.child(bottom).child(txt(
        format!(
            "关注规则关注名称 + 用户（不绑定会复用的 PID）· 日志与偏好自动保存 · {}",
            crate::table_prefs::path().display()
        ),
        9.,
        FontWeight::NORMAL,
        dim(),
    ));
    controls
}

fn scope_button(scope: Scope, active: bool, weak: &WeakEntity<App>) -> Stateful<Div> {
    let weak = weak.clone();
    div()
        .id(("scope", scope as usize))
        .cursor_pointer()
        .px(px(10.))
        .py(px(8.))
        .border(px(1.))
        .border_color(line())
        .bg(if active { text() } else { black() })
        .hover(|this| this.border_color(accent()))
        .on_click(move |_, _, cx| {
            let _ = weak.update(cx, |app, cx| app.set_table_scope(scope, cx));
        })
        .child(caps(
            scope.label(),
            9.5,
            FontWeight::NORMAL,
            if active { black() } else { muted() },
            0.14,
        ))
}

fn state_button(filter: StateFilter, active: bool, weak: &WeakEntity<App>) -> Stateful<Div> {
    let weak = weak.clone();
    div()
        .id(filter.id())
        .cursor_pointer()
        .px(px(7.))
        .py(px(8.))
        .border(px(1.))
        .border_color(if active { accent() } else { line() })
        .bg(if active { accent() } else { black() })
        .hover(|this| this.border_color(accent()))
        .on_click(move |_, _, cx| {
            let _ = weak.update(cx, |app, cx| app.set_table_state(filter, cx));
        })
        .child(caps(
            filter.short(),
            9.,
            FontWeight::NORMAL,
            if active { text() } else { dim() },
            0.12,
        ))
}

fn click_tool(
    id: &'static str,
    label: &str,
    active: bool,
    method: fn(&mut App, &mut Context<App>),
    weak: &WeakEntity<App>,
) -> Stateful<Div> {
    tool_button(id, label, active, {
        let weak = weak.clone();
        Box::new(move |cx: &mut gpui_kit::App| {
            let _ = weak.update(cx, |app, cx| method(app, cx));
        }) as Box<dyn Fn(&mut gpui_kit::App)>
    })
}

fn tool_button(
    id: &'static str,
    label: &str,
    active: bool,
    handler: Box<dyn Fn(&mut gpui_kit::App)>,
) -> Stateful<Div> {
    div()
        .id(id)
        .cursor_pointer()
        .px(px(10.))
        .py(px(7.))
        .border(px(1.))
        .border_color(if active { accent() } else { line_strong() })
        .bg(if active { accent() } else { black() })
        .hover(|this| this.border_color(accent()))
        .on_click(move |_, _, cx| handler(cx))
        .child(caps(
            label,
            9.5,
            FontWeight::NORMAL,
            if active { text() } else { muted() },
            0.12,
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
    overview: &Overview,
    weak: &WeakEntity<App>,
) -> Stateful<Div> {
    let pid = process.pid;
    let sel_weak = weak.clone();
    let menu_press_weak = weak.clone();
    let kill_weak = weak.clone();
    let force_weak = weak.clone();
    let watch_weak = weak.clone();
    let selected = overview.proc_selected == Some(process.pid);
    let watched = overview.table.is_watched(process);
    let change = overview.table.highlight_kind(pid);
    let change_color = match change {
        Some(ChangeKind::State) => warn(),
        Some(ChangeKind::Cpu) => accent(),
        Some(ChangeKind::Memory) => accent_hover(),
        Some(ChangeKind::New) => ok(),
        None => alpha(0x00000000),
    };
    let mut row = h()
        .items_center()
        .id(("proc", index))
        .cursor_pointer()
        .py(px(overview.table.density.padding()))
        .border_b(px(1.))
        .border_color(line_faint())
        .when(selected, |this| this.bg(alpha(0xffffff0c)))
        .hover(|this| this.bg(alpha(0xffffff06)));
    if change.is_some() {
        row = row.child(
            div()
                .w(px(2.))
                .h(px(18.))
                .mr(px(8.))
                .flex_shrink_0()
                .bg(change_color),
        );
    }
    row = row
        .on_click(move |_, _, cx| {
            let _ = sel_weak.update(cx, |app, cx| app.select_process(pid, cx));
        })
        .on_mouse_down(MouseButton::Right, move |event, _, cx| {
            // Open on press, like a native context menu; `on_click` also
            // handles the release so the menu works either way.
            let pos = event.position;
            let _ = menu_press_weak.update(cx, |app, cx| {
                app.toggle_process_menu(pid, pos.x.as_f32(), pos.y.as_f32(), cx)
            });
        });
    let mut name = h()
        .flex_1()
        .min_w_0()
        .items_center()
        .gap(px(6.))
        .child(
            div()
                .id(("watch", index))
                .cursor_pointer()
                .px(px(3.))
                .on_click(move |_, _, cx| {
                    cx.stop_propagation();
                    let _ = watch_weak.update(cx, |app, cx| app.toggle_table_watch(pid, cx));
                })
                .child(txt(
                    if watched { "★" } else { "☆" },
                    13.,
                    FontWeight::NORMAL,
                    if watched { accent() } else { dim() },
                )),
        )
        .child(txt(process.name.clone(), 12.5, FontWeight::NORMAL, text()))
        .child(txt(format!("PID {pid}"), 10.5, FontWeight::NORMAL, dim()))
        .child(txt(process.user.clone(), 10.5, FontWeight::NORMAL, dim()));
    if let Some(kind) = change {
        name = name.child(caps(
            kind.label(),
            8.,
            FontWeight::MEDIUM,
            change_color,
            0.16,
        ));
    }
    row.child(name)
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
                        .border_color(line_strong())
                        .hover(|this| this.border_color(accent()).text_color(accent()))
                        .on_click(move |_, _, cx| {
                            cx.stop_propagation();
                            let _ = kill_weak.update(cx, |app, cx| {
                                app.request_signal(pid, ProcAction::Terminate, cx)
                            });
                        })
                        .child(caps("结束", 9., FontWeight::NORMAL, muted(), 0.12)),
                )
                .child(
                    div()
                        .id(("force", index))
                        .cursor_pointer()
                        .px(px(7.))
                        .py(px(4.))
                        .border(px(1.))
                        .border_color(line_strong())
                        .hover(|this| this.border_color(accent()).text_color(accent()))
                        .on_click(move |_, _, cx| {
                            cx.stop_propagation();
                            let _ = force_weak.update(cx, |app, cx| {
                                app.request_signal(pid, ProcAction::Kill, cx)
                            });
                        })
                        .child(caps("强杀", 9., FontWeight::NORMAL, dim(), 0.12)),
                ),
        )
}

// ============================================================================
// B · cpu
// ============================================================================

/// Chart windows, in samples. Labels are computed at render time from the
/// plugin's real refresh rate, so a 2s cadence correctly reads "240 秒".
const CPU_WINDOWS: [usize; 3] = [120, 60, 30];

fn cpu_window_label(window: usize, refresh_secs: f32) -> String {
    let secs = window as f32 * refresh_secs;
    if secs >= 60. {
        format!("{:.0} 分钟", secs / 60.)
    } else {
        format!("{secs:.0} 秒")
    }
}

fn cpu(overview: &Overview, weak: &WeakEntity<App>) -> Div {
    let snap = overview.proc;
    let sys = overview.snap;
    let history = overview.cpu_history;
    let max = snap.processes.iter().map(|p| p.cpu).fold(1., f32::max);
    let top: Vec<&Process> = snap.processes.iter().take(8).collect();
    let refresh_secs = overview.table.refresh.duration().as_secs_f32();
    let window: usize = overview.cpu_window.min(history.len().max(1));
    let spark: Vec<u8> = history
        .iter()
        .rev()
        .take(window)
        .rev()
        .map(|v| v.clamp(0., 100.) as u8)
        .collect();
    // Sparkline x-positions are spread over the fixed width, so a shorter
    // history fills from the right and grows leftward as samples accumulate.
    let collected = history.len() < window;
    let temp = sys.cpu_right.clone();
    let cores = sys.cores.len();
    let core_hz = if sys.cpu_left.is_empty() {
        String::new()
    } else {
        sys.cpu_left.clone()
    };

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
                                )
                                .child(txt(core_hz, 10.5, FontWeight::NORMAL, dim()).mt(px(6.))),
                        )
                        .child(
                            v().items_end()
                                .child(h().gap(px(6.)).children(CPU_WINDOWS.iter().map(|&span| {
                                    let active = window == span;
                                    let weak = weak.clone();
                                    div()
                                        .id(("cpu-window", span))
                                        .cursor_pointer()
                                        .px(px(7.))
                                        .py(px(3.))
                                        .border(px(1.))
                                        .border_color(if active { accent() } else { line_strong() })
                                        .text_color(if active { accent() } else { dim() })
                                        .hover(|this| {
                                            this.border_color(accent()).text_color(accent())
                                        })
                                        .on_click(move |_, _, cx| {
                                            cx.stop_propagation();
                                            let _ = weak
                                                .update(cx, |app, cx| app.set_cpu_window(span, cx));
                                        })
                                        .child(caps(
                                            &cpu_window_label(span, refresh_secs),
                                            9.,
                                            FontWeight::NORMAL,
                                            if active { accent() } else { dim() },
                                            0.14,
                                        ))
                                })))
                                .child(
                                    txt(
                                        if collected {
                                            format!(
                                                "峰值 {:.0}% · 采集中 {}/{}",
                                                history.iter().fold(0f32, |m, v| m.max(*v)),
                                                history.len(),
                                                window,
                                            )
                                        } else {
                                            format!(
                                                "峰值 {:.0}%",
                                                history.iter().fold(0f32, |m, v| m.max(*v))
                                            )
                                        },
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
                    let spark: Vec<u8> = process.history.clone();
                    v().id(("hot", pid))
                        .cursor_pointer()
                        .py(px(7.))
                        .when(sel, |this| this.bg(alpha(0xffffff0a)))
                        .on_click(move |_, _, cx| {
                            let _ = weak.update(cx, |app, cx| app.focus_process_in_table(pid, cx));
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
                                .child(div().w(px(64.)).h(px(16.)).child(sparkline(
                                    &spark,
                                    64.,
                                    16.,
                                    rgba(0xffffff88).into(),
                                    true,
                                )))
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
                .child(caps("每核占用", 10., FontWeight::NORMAL, dim(), 0.22))
                .children((0..cores).map(|index| {
                    let value = sys.cores.get(index).copied().unwrap_or(0.);
                    let series: Vec<u8> = overview
                        .core_history
                        .get(index)
                        .map(|s| s.iter().map(|v| v.clamp(0., 100.) as u8).collect())
                        .unwrap_or_default();
                    core_line(index, value, &series)
                }))
                .child(div().h(px(1.)).w_full().bg(line()).my(px(22.)))
                .child(caps("系统", 10., FontWeight::NORMAL, dim(), 0.22))
                .child(stat_line(
                    "温度",
                    if temp == "—" || temp.is_empty() {
                        "无传感器".to_string()
                    } else {
                        temp
                    },
                ))
                .child(stat_line("网络", sys.net_right.clone()))
                .child(stat_line(
                    "负载",
                    format!(
                        "{:.2} / {:.2} / {:.2}",
                        snap.load.0, snap.load.1, snap.load.2
                    ),
                ))
                .child(stat_line("上下文切换", format!("{:.0} /秒", sys.ctxt_rate)))
                .child(stat_line("中断", format!("{:.0} /秒", sys.intr_rate)))
                .child(stat_line("可运行", format!("{} 个进程", sys.procs_running)))
                .child(stat_line("线程总数", format!("{}", snap.threads)))
                .child(stat_line("运行时长", overview.snap.detail.clone())),
        )
}

/// One per-core row: label, fill bar, mini sparkline of this core's history.
fn core_line(index: usize, value: f32, series: &[u8]) -> Div {
    let hot = value > 0.6;
    h().items_center()
        .gap(px(10.))
        .py(px(4.))
        .child(txt(format!("CPU{index}"), 10., FontWeight::NORMAL, dim()).w(px(40.)))
        .child(div().flex_1().h(px(1.)).bg(line_strong()).child(
            div().h(px(3.)).w(relative(value.clamp(0., 1.))).bg(if hot {
                accent()
            } else {
                text()
            }),
        ))
        .child(div().w(px(64.)).h(px(14.)).child(sparkline(
            series,
            64.,
            14.,
            rgba(0xffffff66).into(),
            hot,
        )))
        .child(div().w(px(44.)).flex().justify_end().child(txt(
            format!("{:.0}%", value * 100.),
            10.5,
            FontWeight::NORMAL,
            muted(),
        )))
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

/// One treemap cell: a single process, a merged name group, or the leftover
/// tail of the list. `pids` is what a click acts on.
#[derive(Clone)]
struct MemCell {
    label: String,
    pids: Vec<u32>,
    rss_kb: u64,
    /// Signed RSS delta from the last real sample, when it passed the
    /// threshold. `None` means "no change worth flagging".
    growth: Option<i64>,
    rect: Rect,
}

#[derive(Clone, Copy, Default)]
struct Rect {
    x: f32,
    y: f32,
    w: f32,
    h: f32,
}

fn memory(overview: &Overview, weak: &WeakEntity<App>) -> Div {
    let snap = overview.proc;
    let mem = &overview.snap.memory;
    let (cells, listed_kb, matched) = mem_cells(overview);
    let selected = overview
        .proc_selected
        .and_then(|pid| snap.processes.iter().find(|p| p.pid == pid));

    v().flex_1()
        .min_h_0()
        .child(page_head(
            overview,
            "内存",
            format!(
                "{} 个进程匹配 · RSS 合计 {:.1} GB · 面积 = 常驻内存 · 点方块看 PSS/USS",
                matched,
                listed_kb as f32 / 1024. / 1024.,
            ),
        ))
        .child(mem_composition(mem))
        .child(mem_toolbar(overview, weak))
        .child(
            h().flex_1()
                .min_h_0()
                .gap(px(1.))
                .bg(line())
                .border(px(1.))
                .border_color(line())
                .child(
                    v().flex_1()
                        .min_w_0()
                        .min_h_0()
                        .bg(black())
                        .p(px(18.))
                        .child(treemap(cells, weak, overview)),
                )
                .child(mem_detail(overview, selected, weak)),
        )
}

/// Physical memory as one stacked bar, split the way `free` splits it so the
/// segments always add up. The finer breakdown underneath explains *used*; it
/// never invents a number to close a gap.
fn mem_composition(mem: &crate::sys::MemoryBreakdown) -> Div {
    let gb = |kb: u64| kb as f32 / 1024. / 1024.;
    let segment =
        |kb: u64, color: gpui_kit::Hsla| div().h_full().w(relative(mem.frac(kb))).bg(color);
    let legend = |label: &str, kb: u64, color: gpui_kit::Hsla| {
        h().items_center()
            .gap(px(7.))
            .child(div().size(px(7.)).bg(color))
            .child(styled_label(label, dim()))
            .child(styled_label(&format!("{:.1} GB", gb(kb)), muted()))
    };
    // Row of the used-memory breakdown: label, size, share of used.
    let part = |label: &str, kb: u64| {
        h().justify_between()
            .py(px(3.))
            .child(styled_label(label, muted()))
            .child(styled_label(
                &format!(
                    "{:.1} GB · {:.0}%",
                    gb(kb),
                    if mem.used_kb == 0 {
                        0.
                    } else {
                        kb as f32 / mem.used_kb as f32 * 100.
                    }
                ),
                text(),
            ))
    };

    v().mb(px(18.))
        .child(
            h().items_end()
                .justify_between()
                .mb(px(8.))
                .child(
                    h().items_end()
                        .gap(px(10.))
                        .child(txt(
                            format!("{:.1}", gb(mem.used_kb)),
                            26.,
                            FontWeight::EXTRA_LIGHT,
                            text(),
                        ))
                        .child(txt(
                            format!(
                                "/ {:.0} GB 已用 · 可用 {:.1} GB · 空闲 {:.1} GB",
                                gb(mem.total_kb),
                                gb(mem.available_kb),
                                gb(mem.free_kb),
                            ),
                            10.5,
                            FontWeight::NORMAL,
                            muted(),
                        )),
                )
                .child(
                    h().gap(px(14.))
                        .child(legend("已用", mem.used_kb, text()))
                        .child(legend("可回收缓存", mem.reclaimable_kb, accent()))
                        .child(legend("空闲", mem.free_kb, dim())),
                ),
        )
        .child(
            h().h(px(10.))
                .gap(px(1.))
                .bg(line())
                .child(segment(mem.used_kb, text()))
                .child(segment(mem.reclaimable_kb, accent()))
                .child(segment(mem.free_kb, dim())),
        )
        .child(
            h().gap(px(18.))
                .items_center()
                .mt(px(9.))
                .child(styled_label("已用构成", dim()))
                .child(part("应用匿名", mem.anon_kb))
                .child(part("共享内存", mem.shmem_kb))
                .child(part("内核 Slab", mem.slab_kb))
                .child(part("内核栈 + 页表", mem.stack_kb))
                .child(part("Swap 驻留", mem.swap_cached_kb))
                .when(mem.gpu_kb > 0, |this| {
                    this.child(part("GPU 显存", mem.gpu_kb))
                })
                .when(mem.other_kb > 0, |this| {
                    this.child(part("内核未细分", mem.other_kb))
                }),
        )
        .child(
            h().gap(px(18.)).mt(px(6.)).child(styled_label_s(
                &format!(
                    "可回收缓存 = MemAvailable - MemFree，内核随时能丢；Swap {:.1}/{:.0} GB · 脏页 {:.2} GB",
                    gb(mem.swap_used_kb),
                    gb(mem.swap_total_kb),
                    gb(mem.dirty_kb),
                ),
                10.,
                dim(),
            )),
        )
}

fn styled_label(label: &str, color: gpui_kit::Hsla) -> gpui_kit::Div {
    txt(label.to_string(), 10.5, FontWeight::NORMAL, color)
}

fn styled_label_s(label: &str, size: f32, color: gpui_kit::Hsla) -> gpui_kit::Div {
    txt(label.to_string(), size, FontWeight::NORMAL, color)
}

fn mem_toolbar(overview: &Overview, weak: &WeakEntity<App>) -> Div {
    let query = overview.mem_query;
    let group = overview.mem_group;
    let chip = |id: &'static str, label: &str, active: bool, scope: Scope| {
        tool_button(id, label, active, {
            let weak = weak.clone();
            Box::new(move |cx: &mut gpui_kit::App| {
                let _ = weak.update(cx, |app, cx| app.set_mem_scope(scope, cx));
            })
        })
    };
    let group_chip = tool_button(
        "mem-group",
        if group {
            "合并同类 · 开"
        } else {
            "合并同类 · 关"
        },
        group,
        {
            let weak = weak.clone();
            Box::new(move |cx: &mut gpui_kit::App| {
                let _ = weak.update(cx, |app, cx| app.toggle_mem_group(cx));
            })
        },
    );

    h().flex_wrap()
        .items_center()
        .gap(px(6.))
        .mb(px(14.))
        .child(
            h().items_center()
                .w(px(230.))
                .px(px(9.))
                .py(px(5.))
                .gap(px(6.))
                .border(px(1.))
                .border_color(if query.is_empty() {
                    line_strong()
                } else {
                    accent()
                })
                .child(caps("搜索", 8.5, FontWeight::NORMAL, dim(), 0.14))
                .child(if query.is_empty() {
                    styled_label("名称或 PID", dim())
                } else {
                    styled_label(query, text())
                }),
        )
        .child(chip(
            "mem-scope-all",
            Scope::All.label(),
            overview.mem_scope == Scope::All,
            Scope::All,
        ))
        .child(chip(
            "mem-scope-mine",
            Scope::Mine.label(),
            overview.mem_scope == Scope::Mine,
            Scope::Mine,
        ))
        .child(chip(
            "mem-scope-system",
            Scope::System.label(),
            overview.mem_scope == Scope::System,
            Scope::System,
        ))
        .child(group_chip)
        .child(styled_label(
            if query.is_empty() {
                "合并后一块 = 同名进程合计"
            } else {
                "Esc 清空搜索"
            },
            dim(),
        ))
}

/// Build the cells actually drawn: filter, optionally merge by name, sort by
/// RSS, then fold the tail into one honest "rest" block.
fn mem_cells(overview: &Overview) -> (Vec<MemCell>, u64, usize) {
    let snap = overview.proc;
    let query = overview.mem_query.trim().to_lowercase();
    let matched: Vec<&Process> = snap
        .processes
        .iter()
        .filter(|process| match overview.mem_scope {
            Scope::All => true,
            Scope::Mine => process.uid == overview.my_uid,
            Scope::System => process.uid != overview.my_uid,
        })
        .filter(|process| {
            query.is_empty()
                || process.name.to_lowercase().contains(&query)
                || process.pid.to_string().contains(&query)
        })
        .collect();
    let matched_count = matched.len();
    let listed_kb: u64 = matched.iter().map(|p| p.rss_kb).sum();
    let growth_of = |pids: &[u32]| -> Option<i64> {
        let mut total = 0i64;
        let mut any = false;
        for pid in pids {
            if let Some((delta, _)) = overview.mem_growth.get(pid) {
                total += *delta;
                any = true;
            }
        }
        any.then_some(total)
    };

    let mut cells: Vec<MemCell> = Vec::new();
    if overview.mem_group {
        let mut groups: std::collections::HashMap<&str, (Vec<u32>, u64)> =
            std::collections::HashMap::new();
        for process in &matched {
            let entry = groups.entry(process.name.as_str()).or_default();
            entry.0.push(process.pid);
            entry.1 += process.rss_kb;
        }
        for (name, (pids, rss_kb)) in groups {
            cells.push(MemCell {
                label: format!("{name} ×{}", pids.len()),
                growth: growth_of(&pids),
                pids,
                rss_kb,
                rect: Rect::default(),
            });
        }
    } else {
        for process in &matched {
            cells.push(MemCell {
                label: process.name.clone(),
                pids: vec![process.pid],
                rss_kb: process.rss_kb,
                growth: growth_of(&[process.pid]),
                rect: Rect::default(),
            });
        }
    }
    cells.sort_by_key(|cell| std::cmp::Reverse(cell.rss_kb));

    // Small screens and 60+ blocks both hurt: keep the biggest ones legible and
    // be explicit about what is folded away.
    const MAX_CELLS: usize = 40;
    let dropped: u64 = cells.iter().skip(MAX_CELLS).map(|cell| cell.rss_kb).sum();
    cells.truncate(MAX_CELLS);
    if dropped > 0 {
        cells.push(MemCell {
            label: "其余进程".into(),
            pids: Vec::new(),
            rss_kb: dropped,
            growth: None,
            rect: Rect::default(),
        });
    }
    (cells, listed_kb, matched_count)
}

/// The map itself. `container_query` hands back the size the layout actually
/// gave this box, so the treemap fills a wide window instead of a hard-coded
/// 780×420 rectangle.
fn treemap(cells: Vec<MemCell>, weak: &WeakEntity<App>, overview: &Overview) -> impl IntoElement {
    let selected = overview.proc_selected;
    let weak = weak.clone();
    gpui_kit::container_query(move |size, _, _| {
        let w = size.width.as_f32().max(1.);
        let h = size.height.as_f32().max(1.);
        let laid = squarify(&cells, w, h);
        div()
            .relative()
            .size_full()
            .children(laid.into_iter().enumerate().map(|(index, cell)| {
                let is_selected =
                    !cell.pids.is_empty() && cell.pids.iter().any(|pid| Some(*pid) == selected);
                let weak = weak.clone();
                let pids = cell.pids.clone();
                let rss_mb = cell.rss_kb as f32 / 1024.;
                let show_label = cell.rect.w > 78. && cell.rect.h > 26.;
                let show_growth = cell.rect.w > 130. && cell.rect.h > 46.;
                let growth = cell.growth;
                div()
                    .absolute()
                    .id(("mem-cell", index))
                    .cursor_pointer()
                    .left(px(cell.rect.x))
                    .top(px(cell.rect.y))
                    .w(px((cell.rect.w - 1.).max(1.)))
                    .h(px((cell.rect.h - 1.).max(1.)))
                    // Bigger blocks sit brighter, so size reads before the label.
                    .bg(if is_selected {
                        alpha(0xe3193728)
                    } else {
                        alpha(
                            0xffffff08 + (cell.rss_kb.min(4_000_000) / 200_000) as u32 * 0x00000006,
                        )
                    })
                    .border(px(1.))
                    .border_color(if is_selected {
                        accent()
                    } else if growth.is_some_and(|delta| delta > 0) {
                        warn()
                    } else {
                        alpha(0xffffff10)
                    })
                    .hover(|this| this.bg(alpha(0xffffff14)))
                    .on_click(move |_, _, cx| {
                        let _ = weak.update(cx, |app, cx| {
                            if let Some(pid) = pids.first().copied() {
                                app.select_memory_process(pid, cx);
                            }
                        });
                    })
                    .when(show_label, |this| {
                        this.p(px(6.))
                            .child(txt(cell.label.clone(), 10.5, FontWeight::NORMAL, text()))
                            .child(txt(
                                format!("{rss_mb:.0} MB"),
                                9.,
                                FontWeight::NORMAL,
                                dim(),
                            ))
                    })
                    .when_some(growth.filter(|_| show_growth), |this, delta| {
                        this.child(txt(
                            format!(
                                "{}{} MB",
                                if delta > 0 { "↑" } else { "↓" },
                                delta.abs() / 1024
                            ),
                            9.,
                            FontWeight::NORMAL,
                            if delta > 0 { warn() } else { ok() },
                        ))
                    })
            }))
    })
    .size_full()
}

/// Squarified treemap (Bruls, Huizing & van Wijk): keeps cells close to square
/// so labels stay readable and clicks land where they look.
fn squarify(cells: &[MemCell], w: f32, h: f32) -> Vec<MemCell> {
    let total: f64 = cells.iter().map(|cell| cell.rss_kb as f64).sum();
    if total <= 0. || w <= 0. || h <= 0. {
        return Vec::new();
    }
    let area = w as f64 * h as f64;
    let mut items: Vec<(usize, f64)> = cells
        .iter()
        .enumerate()
        .map(|(index, cell)| (index, cell.rss_kb as f64 / total * area))
        .collect();
    items.sort_by(|a, b| b.1.total_cmp(&a.1));

    let mut out = Vec::with_capacity(cells.len());
    let (mut x, mut y, mut cw, mut ch) = (0f64, 0f64, w as f64, h as f64);
    let mut start = 0;
    while start < items.len() {
        let short = cw.min(ch);
        if short <= 0. {
            // No room left at all; anything remaining is unrenderable. A
            // sub-pixel row still gets a rect, because the renderer clamps to
            // one pixel and dropping the cell would hide a real process.
            break;
        }
        let mut end = start + 1;
        let mut row_area = items[start].1;
        let mut row_worst = worst_ratio(&items[start..end], short);
        while end < items.len() {
            let candidate = worst_ratio(&items[start..=end], short);
            if candidate > row_worst {
                break;
            }
            row_area += items[end].1;
            row_worst = candidate;
            end += 1;
        }
        if cw >= ch {
            let thickness = (row_area / ch).min(cw);
            let mut cy = y;
            for &(index, value) in &items[start..end] {
                let height = if row_area > 0. {
                    value / row_area * ch
                } else {
                    0.
                };
                let mut cell = cells[index].clone();
                cell.rect = Rect {
                    x: x as f32,
                    y: cy as f32,
                    w: thickness as f32,
                    h: height as f32,
                };
                out.push(cell);
                cy += height;
            }
            x += thickness;
            cw -= thickness;
        } else {
            let thickness = (row_area / cw).min(ch);
            let mut cx = x;
            for &(index, value) in &items[start..end] {
                let width = if row_area > 0. {
                    value / row_area * cw
                } else {
                    0.
                };
                let mut cell = cells[index].clone();
                cell.rect = Rect {
                    x: cx as f32,
                    y: y as f32,
                    w: width as f32,
                    h: thickness as f32,
                };
                out.push(cell);
                cx += width;
            }
            y += thickness;
            ch -= thickness;
        }
        start = end;
    }
    out
}

/// Aspect ratio of the worst cell in a candidate row; squarify keeps this
/// number as small as it can.
fn worst_ratio(row: &[(usize, f64)], side: f64) -> f64 {
    if row.is_empty() || side <= 0. {
        return f64::MAX;
    }
    let sum: f64 = row.iter().map(|(_, value)| *value).sum();
    let biggest = row.iter().map(|(_, value)| *value).fold(0., f64::max);
    let smallest = row.iter().map(|(_, value)| *value).fold(f64::MAX, f64::min);
    if sum <= 0. || smallest <= 0. {
        return f64::MAX;
    }
    let s2 = sum * sum;
    let side2 = side * side;
    (side2 * biggest / s2).max(s2 / (side2 * smallest))
}

/// Right column: what the selected process really costs.
fn mem_detail(overview: &Overview, selected: Option<&Process>, weak: &WeakEntity<App>) -> Div {
    let Some(process) = selected else {
        return v()
            .w(px(300.))
            .flex_shrink_0()
            .bg(black())
            .p(px(24.))
            .child(caps("选中进程", 10., FontWeight::NORMAL, dim(), 0.22))
            .child(
                txt(
                    "点地图里的方块查看它的内存构成：RSS 细分、PSS、USS 与 Swap。",
                    11.5,
                    FontWeight::NORMAL,
                    muted(),
                )
                .mt(px(10.)),
            );
    };
    let pid = process.pid;
    let growth = overview.mem_growth.get(&pid).map(|(delta, _)| *delta);
    let smaps = overview
        .mem_smaps
        .filter(|_| overview.proc_selected == Some(pid));
    let kb = |value: u64| format!("{:.1} MB", value as f32 / 1024.);
    let row = |label: &str, value: String| {
        h().justify_between()
            .py(px(5.))
            .child(styled_label(label, muted()))
            .child(styled_label(&value, text()))
    };
    let table_weak = weak.clone();
    v().w(px(300.))
        .flex_shrink_0()
        .bg(black())
        .p(px(24.))
        .child(caps("选中进程", 10., FontWeight::NORMAL, dim(), 0.22))
        .child(
            h().items_end()
                .gap(px(8.))
                .mt(px(8.))
                .child(txt(process.name.clone(), 17., FontWeight::LIGHT, text()))
                .child(styled_label(&format!("PID {pid}"), dim())),
        )
        .child(h().gap(px(10.)).mt(px(4.)).child(styled_label(
            &format!("{} · {}", process.user, process.state.label()),
            dim(),
        )))
        .when_some(growth, |this, delta| {
            this.child(h().mt(px(6.)).gap(px(6.)).child(styled_label(
                &format!(
                    "{}{} MB / 4 秒内",
                    if delta > 0 { "↑" } else { "↓" },
                    delta.abs() / 1024
                ),
                if delta > 0 { warn() } else { ok() },
            )))
        })
        .child(div().h(px(1.)).w_full().bg(line()).my(px(16.)))
        .child(caps("RSS 细分", 9.5, FontWeight::NORMAL, dim(), 0.2))
        .child(row("常驻 RSS", kb(process.rss_kb)))
        .child(row("私有匿名", kb(process.rss_anon_kb)))
        .child(row("文件映射", kb(process.rss_file_kb)))
        .child(row("共享内存", kb(process.rss_shmem_kb)))
        .child(row("Swap", kb(process.swap_kb)))
        .child(row("峰值虚拟", kb(process.vm_peak_kb)))
        .child(div().h(px(1.)).w_full().bg(line()).my(px(14.)))
        .child(caps("共享页真实占用", 9.5, FontWeight::NORMAL, dim(), 0.2))
        .child(match smaps {
            Some(smaps) => v()
                .child(row("PSS 按比例", kb(smaps.pss_kb)))
                .child(row("USS 独占", kb(smaps.uss_kb)))
                .child(row("共享页", kb(smaps.shared_kb)))
                .child(row("Swap PSS", kb(smaps.swap_pss_kb)))
                .child(
                    styled_label_s(
                        "PSS 把共享页按映射进程数摊开，比 RSS 更接近真实占用。",
                        9.5,
                        dim(),
                    )
                    .mt(px(6.)),
                ),
            None => v().child(
                styled_label_s("读不到 smaps_rollup（进程已退出或无权限）", 10., dim()).mt(px(4.)),
            ),
        })
        .child(div().h(px(1.)).w_full().bg(line()).my(px(14.)))
        .child(
            h().id("mem-open-table")
                .cursor_pointer()
                .justify_center()
                .py(px(7.))
                .border(px(1.))
                .border_color(line_strong())
                .hover(|this| this.border_color(accent()).text_color(accent()))
                .on_click(move |_, _, cx| {
                    let _ = table_weak.update(cx, |app, cx| app.focus_process_in_table(pid, cx));
                })
                .child(caps(
                    "在 Table 中打开",
                    9.5,
                    FontWeight::NORMAL,
                    muted(),
                    0.14,
                )),
        )
}

/// Floating context menu + modal confirmation + toast, drawn above the table.
///
/// The root window owns keyboard shortcuts; these overlays only own their own
/// mouse interactions. `ProcOverlays` is owned data so callbacks never borrow
/// the render stack.
pub fn overlays(state: ProcOverlays, weak: &WeakEntity<App>) -> Div {
    let ProcOverlays {
        dialog,
        menu,
        toast,
        selected,
        paused,
        frozen,
        viewport,
        menu_name,
        menu_watched,
        log_open,
        log,
        log_path,
        table_error,
        log_led,
    } = state;
    // A zero-footprint container. The overlays themselves are absolutely
    // positioned against the window root (the only `relative()` ancestor), so
    // this wrapper must NOT be full-size: a full-size hitbox here would sit
    // above the page and swallow every click on the table behind it.
    let mut root = div();

    if let Some(dialog) = dialog {
        root = root.child(confirm_dialog(dialog, weak));
    } else if let Some((pid, x, y)) = menu {
        let (w, h) = viewport;
        let menu_w = 224.;
        let menu_h = 430.;
        let left = x.clamp(8., (w - menu_w - 12.).max(8.));
        let top = y.clamp(8., (h - menu_h - 12.).max(8.));
        // No full-screen backdrop: an occluding scrim would receive the
        // mouse-down before a menu item's click listener fires and close the
        // menu too early. `context_menu` closes itself on any outside press.
        root = root.child(context_menu(
            pid,
            &menu_name,
            left,
            top,
            paused,
            frozen,
            selected,
            menu_watched,
            weak,
        ));
    }

    if log_open {
        root = root.child(log_drawer(
            &log,
            &log_path,
            table_error.as_deref(),
            log_led,
            weak,
        ));
    }

    if let Some(toast) = toast {
        root = root.child(
            div()
                .id("proc-toast")
                .absolute()
                .bottom(px(22.))
                .left(px(88.))
                .px(px(14.))
                .py(px(9.))
                .bg(surface_2())
                .border(px(1.))
                .border_color(line_strong())
                .child(txt(toast, 10.5, FontWeight::NORMAL, text())),
        );
    }
    root
}

/// Modal-ish confirmation: dark scrim + centered card. Every destructive
/// signal goes through this; the old "click the same button twice" is gone.
fn confirm_dialog(dialog: crate::ProcDialog, weak: &WeakEntity<App>) -> Stateful<Div> {
    let confirm = weak.clone();
    let cancel = weak.clone();
    let scrim = weak.clone();
    let action = dialog.action;
    let danger = action.danger();
    h().id("confirm-scrim")
        .absolute()
        .top(px(0.))
        .left(px(0.))
        .size_full()
        .bg(alpha(0x000000b8))
        .occlude()
        .justify_center()
        .items_center()
        .on_mouse_down(MouseButton::Left, move |_, _, cx| {
            let _ = scrim.update(cx, |app, cx| app.cancel_signal(cx));
        })
        .child(
            v().id("confirm-card")
                .w(px(440.))
                .bg(black())
                .border(px(1.))
                .border_color(if danger { accent() } else { line_strong() })
                .p(px(28.))
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .child(caps("确认操作", 10., FontWeight::NORMAL, dim(), 0.26))
                .child(txt(action.title(), 24., FontWeight::LIGHT, text()).mt(px(12.)))
                .child(
                    txt(
                        format!("{} · PID {}", dialog.name, dialog.pid),
                        11.,
                        FontWeight::NORMAL,
                        muted(),
                    )
                    .mt(px(6.)),
                )
                .child(
                    div()
                        .mt(px(18.))
                        .p(px(12.))
                        .bg(surface_2())
                        .border(px(1.))
                        .border_color(line())
                        .child(caps("命令行", 9., FontWeight::NORMAL, dim(), 0.2))
                        .child(
                            txt(
                                if dialog.command.is_empty() {
                                    "—（内核线程 / 已退出）".to_string()
                                } else {
                                    dialog.command.clone()
                                },
                                10.5,
                                FontWeight::NORMAL,
                                muted(),
                            )
                            .whitespace_normal()
                            .line_height(px(16.))
                            .mt(px(7.)),
                        ),
                )
                .child(
                    txt(
                        action.warning(),
                        10.5,
                        FontWeight::NORMAL,
                        if danger { warn() } else { muted() },
                    )
                    .whitespace_normal()
                    .line_height(px(16.))
                    .mt(px(16.)),
                )
                .child(
                    h().gap(px(10.))
                        .mt(px(24.))
                        .child(dialog_button(
                            "confirm-cancel",
                            "取消 · Esc",
                            false,
                            Box::new(move |cx: &mut GpuiApp| {
                                let _ = cancel.update(cx, |app, cx| app.cancel_signal(cx));
                            }),
                        ))
                        .child(dialog_button(
                            "confirm-ok",
                            action.verb(),
                            danger,
                            Box::new(move |cx: &mut GpuiApp| {
                                let _ = confirm.update(cx, |app, cx| app.confirm_signal(cx));
                            }),
                        )),
                )
                .child(txt("Enter 确认 · Esc 取消", 9.5, FontWeight::NORMAL, dim()).mt(px(14.))),
        )
}

fn dialog_button(
    id: &'static str,
    label: &'static str,
    danger: bool,
    handler: Box<dyn Fn(&mut GpuiApp)>,
) -> Stateful<Div> {
    h().id(id)
        .flex_1()
        .cursor_pointer()
        .justify_center()
        .items_center()
        .py(px(11.))
        .bg(if danger { accent() } else { surface_2() })
        .border(px(1.))
        .border_color(if danger { accent() } else { line_strong() })
        .hover(|this| this.border_color(text()))
        .on_click(move |_, _, cx| {
            cx.stop_propagation();
            handler(cx);
        })
        .child(caps(
            label,
            10.,
            FontWeight::NORMAL,
            if danger { text() } else { muted() },
            0.14,
        ))
}

fn context_menu(
    pid: u32,
    name: &str,
    left: f32,
    top: f32,
    paused: bool,
    frozen: Option<u32>,
    selected: Option<u32>,
    watched: bool,
    weak: &WeakEntity<App>,
) -> Stateful<Div> {
    let focus = weak.clone();
    let copy_row = weak.clone();
    let copy_cmd = weak.clone();
    let pause = weak.clone();
    let freeze = weak.clone();
    let terminate = weak.clone();
    let kill = weak.clone();
    let stop = weak.clone();
    let cont = weak.clone();
    let watch = weak.clone();
    let close = weak.clone();
    v().id("proc-menu")
        .absolute()
        .left(px(left))
        .top(px(top))
        .w(px(224.))
        .bg(black())
        .border(px(1.))
        .border_color(line_strong())
        .occlude()
        .p(px(6.))
        .on_mouse_down_out(move |_, _, cx| {
            let _ = close.update(cx, |app, cx| app.close_process_menu(cx));
        })
        .child(
            v().px(px(10.))
                .py(px(8.))
                .border_b(px(1.))
                .border_color(line())
                .mb(px(4.))
                .child(txt(name.to_string(), 12., FontWeight::NORMAL, text()))
                .child(txt(format!("PID {pid}"), 9.5, FontWeight::NORMAL, dim()).mt(px(3.))),
        )
        .child(menu_item(
            "menu-focus",
            "查看详情 / 保持选中",
            "点击",
            false,
            {
                let weak = focus.clone();
                Box::new(move |cx: &mut GpuiApp| {
                    let _ = weak.update(cx, |app, cx| app.focus_process_detail(pid, cx));
                })
            },
        ))
        .child(menu_item(
            "menu-copy-row",
            "复制进程行",
            "Ctrl+C",
            false,
            {
                Box::new(move |cx: &mut GpuiApp| {
                    let _ = copy_row.update(cx, |app, cx| app.copy_process_row(pid, cx));
                })
            },
        ))
        .child(menu_item(
            "menu-copy-cmd",
            "复制命令行",
            "Ctrl+Shift+C",
            false,
            {
                Box::new(move |cx: &mut GpuiApp| {
                    let _ = copy_cmd.update(cx, |app, cx| app.copy_process_command(pid, cx));
                })
            },
        ))
        .child(menu_item(
            "menu-watch",
            if watched {
                "取消关注 · 移出置顶"
            } else {
                "关注此进程 · 置顶"
            },
            "Ctrl+W",
            false,
            {
                Box::new(move |cx: &mut GpuiApp| {
                    let _ = watch.update(cx, |app, cx| app.toggle_table_watch(pid, cx));
                })
            },
        ))
        .child(menu_separator())
        .child(menu_item(
            "menu-pause",
            if paused {
                "继续刷新列表"
            } else {
                "暂停刷新列表"
            },
            "Ctrl+Space",
            false,
            {
                Box::new(move |cx: &mut GpuiApp| {
                    let _ = pause.update(cx, |app, cx| app.toggle_table_pause(cx));
                })
            },
        ))
        .child(menu_item(
            "menu-freeze",
            if frozen.is_some() {
                "取消排序冻结"
            } else {
                "排序冻结此行"
            },
            "Ctrl+F",
            false,
            {
                Box::new(move |cx: &mut GpuiApp| {
                    let _ = freeze.update(cx, |app, cx| {
                        if selected == Some(pid) || frozen.is_some() {
                            app.toggle_table_freeze(cx)
                        } else {
                            app.focus_process_detail(pid, cx);
                            app.toggle_table_freeze(cx)
                        }
                    });
                })
            },
        ))
        .child(menu_separator())
        .child(menu_item(
            "menu-stop",
            "暂停进程 · SIGSTOP",
            "Ctrl+P",
            false,
            {
                Box::new(move |cx: &mut GpuiApp| {
                    let _ =
                        stop.update(cx, |app, cx| app.request_signal(pid, ProcAction::Stop, cx));
                })
            },
        ))
        .child(menu_item(
            "menu-cont",
            "继续进程 · SIGCONT",
            "Ctrl+Shift+P",
            false,
            {
                Box::new(move |cx: &mut GpuiApp| {
                    let _ = cont.update(cx, |app, cx| {
                        app.request_signal(pid, ProcAction::Continue, cx)
                    });
                })
            },
        ))
        .child(menu_separator())
        .child(menu_item(
            "menu-term",
            "结束 · SIGTERM",
            "Ctrl+K",
            true,
            {
                Box::new(move |cx: &mut GpuiApp| {
                    let _ = terminate.update(cx, |app, cx| {
                        app.request_signal(pid, ProcAction::Terminate, cx)
                    });
                })
            },
        ))
        .child(menu_item(
            "menu-kill",
            "强制结束 · SIGKILL",
            "Ctrl+Shift+K",
            true,
            {
                Box::new(move |cx: &mut GpuiApp| {
                    let _ =
                        kill.update(cx, |app, cx| app.request_signal(pid, ProcAction::Kill, cx));
                })
            },
        ))
}

fn menu_item(
    id: &'static str,
    label: &'static str,
    shortcut: &'static str,
    danger: bool,
    handler: Box<dyn Fn(&mut GpuiApp)>,
) -> Stateful<Div> {
    h().id(id)
        .cursor_pointer()
        .px(px(10.))
        .py(px(7.))
        .justify_between()
        .items_center()
        .hover(|this| this.bg(alpha(0xffffff0e)))
        .on_click(move |_, _, cx| {
            cx.stop_propagation();
            handler(cx);
        })
        .child(txt(
            label,
            10.5,
            FontWeight::NORMAL,
            if danger { warn() } else { text() },
        ))
        .child(txt(shortcut, 8.5, FontWeight::NORMAL, dim()))
}

fn menu_separator() -> Div {
    div().h(px(1.)).my(px(5.)).bg(line())
}

/// `26 · 操作日志`: a real, persistent journal of signals, watch rules,
/// refresh-rate changes, pause and freeze. Empty until something happened.
fn log_drawer(
    log: &[crate::table_prefs::Entry],
    path: &str,
    error: Option<&str>,
    led: &crate::scroll::Led,
    weak: &WeakEntity<App>,
) -> Stateful<Div> {
    let scrim_close = weak.clone();
    let button_close = weak.clone();
    let copy = weak.clone();
    let mut list = v().gap(px(1.));
    if log.is_empty() {
        list = list.child(
            v().py(px(28.))
                .child(txt("还没有操作记录", 12., FontWeight::NORMAL, muted()))
                .child(
                    txt(
                        "发送信号、关注进程或调整刷新频率后，这里会留下时间戳。",
                        10.,
                        FontWeight::NORMAL,
                        dim(),
                    )
                    .mt(px(8.)),
                ),
        );
    } else {
        for (index, entry) in log.iter().enumerate() {
            list = list.child(
                h().id(("log", index))
                    .items_center()
                    .gap(px(10.))
                    .py(px(7.))
                    .border_b(px(1.))
                    .border_color(line_faint())
                    .child(txt(entry.at.clone(), 9.5, FontWeight::NORMAL, dim()))
                    .child(div().w(px(62.)).flex_shrink_0().child(caps(
                        entry.kind.as_str(),
                        8.5,
                        FontWeight::MEDIUM,
                        muted(),
                        0.14,
                    )))
                    .child(
                        txt(
                            entry.detail.clone(),
                            10.5,
                            FontWeight::NORMAL,
                            if entry.ok { text() } else { warn() },
                        )
                        .flex_1()
                        .min_w_0()
                        .text_ellipsis(),
                    ),
            );
        }
    }
    h().id("log-scrim")
        .absolute()
        .top(px(0.))
        .left(px(0.))
        .size_full()
        .bg(alpha(0x000000b8))
        .occlude()
        .justify_end()
        .items_stretch()
        .on_mouse_down(MouseButton::Left, move |_, _, cx| {
            let _ = scrim_close.update(cx, |app, cx| app.toggle_proc_log(cx));
        })
        .child(
            v().id("log-panel")
                .w(px(560.))
                .h_full()
                .bg(black())
                .border_l(px(1.))
                .border_color(line_strong())
                .p(px(26.))
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .child(
                    h().justify_between()
                        .items_center()
                        .child(caps(
                            "操作日志 · 最近 200 条",
                            10.,
                            FontWeight::NORMAL,
                            dim(),
                            0.24,
                        ))
                        .child(
                            div()
                                .id("log-close")
                                .cursor_pointer()
                                .px(px(10.))
                                .py(px(5.))
                                .border(px(1.))
                                .border_color(line_strong())
                                .on_click(move |_, _, cx| {
                                    cx.stop_propagation();
                                    let _ =
                                        button_close.update(cx, |app, cx| app.toggle_proc_log(cx));
                                })
                                .child(caps("关闭 · Esc", 9., FontWeight::NORMAL, muted(), 0.12)),
                        ),
                )
                .child(
                    txt(
                        format!("自动保存到 {}", path),
                        9.5,
                        FontWeight::NORMAL,
                        dim(),
                    )
                    .mt(px(7.)),
                )
                .when_some(error, |this, error| {
                    this.child(txt(error.to_string(), 9.5, FontWeight::NORMAL, warn()).mt(px(7.)))
                })
                .child(h().gap(px(8.)).mt(px(16.)).child(detail_button(
                    ("log-copy", 0),
                    "复制日志 TSV",
                    false,
                    Box::new(move |cx: &mut GpuiApp| {
                        let _ = copy.update(cx, |app, cx| app.copy_table_log(cx));
                    }),
                )))
                .child(
                    v().relative()
                        .flex_1()
                        .min_h_0()
                        .mt(px(16.))
                        .child(
                            v().id("log-rows")
                                .absolute()
                                .top(px(0.))
                                .left(px(0.))
                                .right(px(22.))
                                .bottom(px(0.))
                                .overflow_y_scroll()
                                .track_scroll(led.handle())
                                .child(list),
                        )
                        .child(crate::led_column(
                            "led-log",
                            led,
                            0.,
                            crate::scroll::Target::Log,
                            weak,
                        )),
                ),
        )
}

fn detail_panel(
    _overview: &Overview,
    process: Option<&Process>,
    weak: &WeakEntity<App>,
    title: &str,
    rich: bool,
) -> Div {
    let Some(process) = process else {
        return v().w(px(300.)).bg(black()).p(px(24.));
    };
    let pid = process.pid;
    let terminate = weak.clone();
    let kill = weak.clone();
    let stop = weak.clone();
    let cont = weak.clone();
    let copy_row = weak.clone();
    let copy_cmd = weak.clone();
    let watch = weak.clone();
    let log = weak.clone();
    let log_focus = weak.clone();
    let watched = _overview.table.is_watched(process);
    let command = {
        let cmd = proc::command(pid);
        if cmd.is_empty() {
            "—（内核线程）".to_string()
        } else {
            cmd
        }
    };
    let mut panel = v()
        .w(px(300.))
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
                    txt(command, 10.5, FontWeight::NORMAL, muted())
                        .line_height(px(15.))
                        .mt(px(6.)),
                ),
        )
        .child(
            h().gap(px(8.))
                .mt(px(20.))
                .child(detail_button(
                    ("detail-kill", pid as usize),
                    "结束",
                    true,
                    Box::new(move |cx: &mut GpuiApp| {
                        let _ = terminate.update(cx, |app, cx| {
                            app.request_signal(pid, ProcAction::Terminate, cx)
                        });
                    }),
                ))
                .child(detail_button(
                    ("detail-force", pid as usize),
                    "强杀",
                    true,
                    Box::new(move |cx: &mut GpuiApp| {
                        let _ = kill
                            .update(cx, |app, cx| app.request_signal(pid, ProcAction::Kill, cx));
                    }),
                )),
        );

    if rich {
        panel = panel
            .child(
                h().gap(px(8.))
                    .mt(px(8.))
                    .child(detail_button(
                        ("detail-watch", pid as usize),
                        if watched {
                            "取消关注"
                        } else {
                            "关注 · 置顶"
                        },
                        false,
                        Box::new(move |cx: &mut GpuiApp| {
                            let _ = watch.update(cx, |app, cx| app.toggle_table_watch(pid, cx));
                        }),
                    ))
                    .child(detail_button(
                        ("detail-log", pid as usize),
                        "操作日志",
                        false,
                        Box::new(move |cx: &mut GpuiApp| {
                            let _ = log.update(cx, |app, cx| app.toggle_proc_log(cx));
                        }),
                    )),
            )
            .child(
                h().gap(px(8.))
                    .mt(px(8.))
                    .child(detail_button(
                        ("detail-stop", pid as usize),
                        "暂停",
                        false,
                        Box::new(move |cx: &mut GpuiApp| {
                            let _ = stop.update(cx, |app, cx| {
                                app.request_signal(pid, ProcAction::Stop, cx)
                            });
                        }),
                    ))
                    .child(detail_button(
                        ("detail-cont", pid as usize),
                        "继续",
                        false,
                        Box::new(move |cx: &mut GpuiApp| {
                            let _ = cont.update(cx, |app, cx| {
                                app.request_signal(pid, ProcAction::Continue, cx)
                            });
                        }),
                    )),
            )
            .child(
                h().gap(px(8.))
                    .mt(px(8.))
                    .child(detail_button(
                        ("detail-copy-row", pid as usize),
                        "复制行",
                        false,
                        Box::new(move |cx: &mut GpuiApp| {
                            let _ = copy_row.update(cx, |app, cx| app.copy_process_row(pid, cx));
                        }),
                    ))
                    .child(detail_button(
                        ("detail-copy-cmd", pid as usize),
                        "复制命令行",
                        false,
                        Box::new(move |cx: &mut GpuiApp| {
                            let _ =
                                copy_cmd.update(cx, |app, cx| app.copy_process_command(pid, cx));
                        }),
                    )),
            )
            .child(
                div()
                    .id(("detail-focus", pid as usize))
                    .mt(px(14.))
                    .cursor_pointer()
                    .py(px(8.))
                    .border_t(px(1.))
                    .border_color(line())
                    .on_click(move |_, _, cx| {
                        let _ = log_focus.update(cx, |app, cx| app.focus_process_detail(pid, cx));
                    })
                    .child(txt(
                        "右键菜单里有更多操作 · 点击保持选中",
                        9.5,
                        FontWeight::NORMAL,
                        dim(),
                    )),
            );
    }
    panel
}

fn detail_button(
    id: (&'static str, usize),
    label: &'static str,
    danger: bool,
    handler: Box<dyn Fn(&mut GpuiApp)>,
) -> Stateful<Div> {
    h().id(id)
        .flex_1()
        .cursor_pointer()
        .justify_center()
        .items_center()
        .py(px(9.))
        .border(px(1.))
        .border_color(if danger { line_strong() } else { line() })
        .hover(|this| this.border_color(accent()).text_color(accent()))
        .on_click(move |_, _, cx| handler(cx))
        .child(caps(
            label,
            9.5,
            FontWeight::NORMAL,
            if danger { muted() } else { dim() },
            0.14,
        ))
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

#[cfg(test)]
mod tests {
    use super::*;

    fn cell(rss_kb: u64) -> MemCell {
        MemCell {
            label: format!("p{rss_kb}"),
            pids: Vec::new(),
            rss_kb,
            growth: None,
            rect: Rect::default(),
        }
    }

    #[test]
    fn squarify_fills_the_container_without_overlap() {
        let cells: Vec<MemCell> = [900u64, 500, 300, 200, 100, 50, 25]
            .into_iter()
            .map(cell)
            .collect();
        let (w, h) = (800., 500.);
        let laid = squarify(&cells, w, h);
        assert_eq!(laid.len(), cells.len());

        // Areas add up to the container, so no cell is lost or double counted.
        let area: f64 = laid.iter().map(|c| c.rect.w as f64 * c.rect.h as f64).sum();
        assert!((area - (w as f64 * h as f64)).abs() < 1.0, "area {area}");

        // Every rect stays inside the container.
        for c in &laid {
            assert!(c.rect.x >= -0.01 && c.rect.y >= -0.01);
            assert!(c.rect.x + c.rect.w <= w + 0.5);
            assert!(c.rect.y + c.rect.h <= h + 0.5);
        }

        // A cell's share of the area tracks its share of the RSS total, which
        // is what makes "面积 = 常驻内存" honest.
        let total: f64 = cells.iter().map(|c| c.rss_kb as f64).sum();
        for c in &laid {
            let expected = c.rss_kb as f64 / total;
            let actual = (c.rect.w as f64 * c.rect.h as f64) / (w as f64 * h as f64);
            assert!((expected - actual).abs() < 0.01, "{expected} vs {actual}");
        }
    }

    #[test]
    fn squarify_handles_degenerate_input() {
        assert!(squarify(&[], 100., 100.).is_empty());
        let single = squarify(&[cell(10)], 100., 50.);
        assert_eq!(single.len(), 1);
        assert!((single[0].rect.w - 100.).abs() < 0.5);
        assert!((single[0].rect.h - 50.).abs() < 0.5);
        // Zero total would divide by zero; it must return nothing instead.
        assert!(squarify(&[cell(0)], 100., 100.).is_empty());
    }

    #[test]
    fn squarify_skips_cells_too_small_to_draw() {
        // One dominant cell plus a speck: the speck must not produce NaN.
        let laid = squarify(&[cell(10_000_000), cell(1)], 400., 300.);
        assert_eq!(laid.len(), 2);
        for c in &laid {
            assert!(c.rect.w.is_finite() && c.rect.h.is_finite());
            assert!(c.rect.w >= 0. && c.rect.h >= 0.);
        }
    }
}
