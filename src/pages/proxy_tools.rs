//! Renderer for the `proxy-tools` page-level extension.
//!
//! Layout mirrors the other diagnostic pages: a conclusion strip driven by real
//! facts, the three risk faces on the left, and verbatim backend stdout on the
//! right. Everything shown here comes from [`crate::proxy_tools::StatusFacts`];
//! nothing is parsed back out of the text panel.

use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    App as GpuiApp, Div, FontWeight, Hsla, InteractiveElement as _, ParentElement as _, Stateful,
    StatefulInteractiveElement as _, Styled, WeakEntity, div, px,
};

use crate::proxy_tools::{self, Op, StatusFacts, Tab};
use crate::theme::{
    accent, accent_hover, alpha, black, caps, dim, h, line, line_strong, muted, ok, surface_2,
    text, txt, v, warn,
};
use crate::{App, Overview};

pub fn body(overview: &Overview, weak: &WeakEntity<App>) -> Div {
    v().child(hero(overview, weak))
        .child(metrics(overview))
        .child(
            h().w_full()
                .items_end()
                .justify_between()
                .mb(px(20.))
                .child(caps("诊断台", 12., FontWeight::MEDIUM, muted(), 0.24))
                .child(caps(
                    "内建后端 stdout · 未格式化 · 未裁剪",
                    11.,
                    FontWeight::NORMAL,
                    dim(),
                    0.12,
                )),
        )
        .child(console(overview, weak))
        .child(fields(overview))
        .child(footer())
}

fn status_color(facts: Option<&StatusFacts>) -> Hsla {
    match facts.map(|facts| facts.status.as_str()) {
        Some("healthy") => ok(),
        Some("attention") => warn(),
        Some("degraded") => accent(),
        _ => muted(),
    }
}

fn hero(overview: &Overview, weak: &WeakEntity<App>) -> Div {
    let state = overview.proxy;
    let mut actions = h().items_center().gap(px(8.));
    for op in Op::ALL {
        let weak = weak.clone();
        let armed = state.armed == Some(op);
        let label = if armed {
            match op {
                Op::Apply => "确认 APPLY",
                Op::Revert => "确认 REVERT",
                _ => op.label(),
            }
        } else {
            op.label()
        };
        actions = actions.child(button(
            label,
            op == Op::Status || armed,
            op == Op::Handover || armed && op != Op::Status,
            state.busy,
            move |cx| {
                let _ = weak.update(cx, |app, cx| app.run_proxy_tools(op, cx));
            },
        ));
    }
    v().mb(px(44.))
        .child(
            caps(
                "自研扩展 · proxy-tools · v0.1.0",
                11.,
                FontWeight::NORMAL,
                dim(),
                0.26,
            )
            .mb(px(14.)),
        )
        .child(
            h().w_full()
                .items_end()
                .justify_between()
                .gap(px(24.))
                .child(
                    v().flex_1()
                        .min_w_0()
                        .child(
                            txt("代理工具", 44., FontWeight::EXTRA_LIGHT, text())
                                .line_height(px(46.)),
                        )
                        .child(
                            txt(
                                "本机中继与控制面诊断台：只读探测真实端口 / 单元 / 环境；GNOUS 只写自己的文件，绝不启停承载本机联网的旧中继。",
                                13.,
                                FontWeight::NORMAL,
                                muted(),
                            )
                            .mt(px(12.)),
                        ),
                )
                .child(actions),
        )
}

fn button(
    label: &'static str,
    solid: bool,
    danger: bool,
    disabled: bool,
    handler: impl Fn(&mut GpuiApp) + 'static,
) -> Stateful<Div> {
    let (border, bg, fg) = if solid {
        (accent(), accent(), text())
    } else if danger {
        (accent(), black(), accent())
    } else {
        (line_strong(), black(), muted())
    };
    let id = format!("proxy-tools-action-{label}");
    div()
        .id(id)
        .cursor_pointer()
        .px(px(18.))
        .py(px(11.))
        .border(px(1.))
        .border_color(border)
        .bg(bg)
        .when(disabled, |this| this.opacity(0.45))
        .hover(|this| {
            if solid {
                this.bg(accent_hover())
            } else {
                this.bg(alpha(0xffffff0a))
            }
        })
        .child(caps(label, 10., FontWeight::MEDIUM, fg, 0.16))
        .on_click(move |_, _, cx| {
            if !disabled {
                handler(cx);
            }
        })
}

fn metrics(overview: &Overview) -> Div {
    let facts = overview.proxy.facts.clone().unwrap_or_default();
    let checks_ok = facts.checks.iter().filter(|check| check.ok).count();
    let owner = match facts.listen_owner.as_str() {
        "gnous" => "GNOUS",
        "legacy" => "LEGACY",
        "other" => "OTHER",
        _ => "—",
    };
    let values = [
        (
            "代理结论",
            if facts.status.is_empty() {
                "—".to_string()
            } else {
                facts.status.to_uppercase()
            },
            String::new(),
            status_color(overview.proxy.facts.as_ref()),
        ),
        ("端口归属", owner.to_string(), String::new(), text()),
        (
            "上游延迟",
            if facts.upstream.reachable {
                facts.upstream.latency_ms.to_string()
            } else {
                "—".to_string()
            },
            "ms".to_string(),
            if facts.upstream.reachable {
                text()
            } else {
                accent()
            },
        ),
        (
            "诊断项",
            format!("{checks_ok}"),
            format!("/ {}", facts.checks.len()),
            text(),
        ),
    ];
    let mut row = h()
        .w_full()
        .items_stretch()
        .border_t(px(1.))
        .border_b(px(1.))
        .border_color(line());
    for (index, (label, value, unit, color)) in values.into_iter().enumerate() {
        row = row.child(
            v().flex_1()
                .min_w_0()
                .py(px(24.))
                .when(index > 0, |this| {
                    this.pl(px(26.)).border_l(px(1.)).border_color(line())
                })
                .child(caps(label, 10., FontWeight::NORMAL, dim(), 0.22))
                .child(
                    h().items_end()
                        .gap(px(8.))
                        .mt(px(16.))
                        .child(
                            txt(
                                value,
                                if index == 0 { 26. } else { 44. },
                                FontWeight::EXTRA_LIGHT,
                                color,
                            )
                            .line_height(px(46.)),
                        )
                        .when(!unit.is_empty(), |this| {
                            this.child(txt(unit, 13., FontWeight::LIGHT, muted()))
                        }),
                ),
        );
    }
    row
}

/// One entry per face: (title, description, [(item, ok, detail)]).
fn faces(facts: &StatusFacts) -> Vec<(&'static str, &'static str, Vec<(String, bool, String)>)> {
    vec![
        (
            "诊断 · 只读",
            "端口、上游、内网排除项；不改任何状态。",
            vec![
                (
                    format!("端口 {}", facts.listen),
                    facts.listening,
                    if facts.listening {
                        format!("监听者：{}", facts.listen_owner)
                    } else {
                        "没有进程监听".into()
                    },
                ),
                (
                    format!("上游 {}", facts.upstream.addr),
                    facts.upstream.reachable,
                    format!(
                        "{} · {} ms",
                        if facts.upstream.reachable {
                            "可达"
                        } else {
                            "不可达"
                        },
                        facts.upstream.latency_ms
                    ),
                ),
                (
                    "no_proxy 覆盖".to_string(),
                    facts.no_proxy.missing_count == 0,
                    format!(
                        "{} 条 · {}",
                        facts.no_proxy.entries,
                        if facts.no_proxy.missing_count == 0 {
                            "完整".to_string()
                        } else {
                            format!("缺 {}", facts.no_proxy.missing_count)
                        }
                    ),
                ),
            ],
        ),
        (
            "数据面 · GNOUS 中继",
            "Rust std 实现，随 GNOUS 版本安装；只启停自己的单元。",
            vec![
                (
                    "单元文件".to_string(),
                    facts.relay.present,
                    if facts.relay.present {
                        facts.paths.unit.clone()
                    } else {
                        "未安装（APPLY 只写文件）".into()
                    },
                ),
                (
                    "单元运行".to_string(),
                    facts.relay.running(),
                    if facts.relay.running() {
                        format!("{} / {}", facts.relay.sub, facts.relay.enabled)
                    } else {
                        "未运行".into()
                    },
                ),
                (
                    "端口交棒".to_string(),
                    facts.listen_owner == "gnous",
                    if facts.listen_owner == "gnous" {
                        "已由 GNOUS 承担".into()
                    } else {
                        "仍在旧中继手上（HANDOVER 只打印命令）".into()
                    },
                ),
            ],
        ),
        (
            "控制面 · 只读展示",
            "gsettings / environment.d 由脚本维护；GNOUS 不改写。",
            vec![
                (
                    "GNOME 模式".to_string(),
                    facts.control.gnome_mode.is_some(),
                    facts
                        .control
                        .gnome_mode
                        .clone()
                        .unwrap_or_else(|| "读取失败".into()),
                ),
                (
                    "environment.d".to_string(),
                    facts.control.env_keys > 0,
                    format!(
                        "{} 个文件 · {} 条键",
                        facts.control.env_files.len(),
                        facts.control.env_keys
                    ),
                ),
                (
                    "user manager".to_string(),
                    facts.control.user_manager_keys > 0,
                    format!("{} 条代理变量", facts.control.user_manager_keys),
                ),
            ],
        ),
    ]
}

fn console(overview: &Overview, weak: &WeakEntity<App>) -> Div {
    h().w_full()
        .items_stretch()
        .h(px(CONSOLE_HEIGHT))
        .border(px(1.))
        .border_color(line())
        .child(faces_column(overview, weak))
        .child(output_side(overview, weak))
}

fn faces_column(overview: &Overview, weak: &WeakEntity<App>) -> Div {
    let state = overview.proxy;
    let facts = state.facts.clone().unwrap_or_default();
    let mut column = v()
        .w(px(360.))
        .flex_shrink_0()
        .border_r(px(1.))
        .border_color(line());
    let entries = faces(&facts);
    let last = entries.len().saturating_sub(1);
    for (index, (title, description, items)) in entries.into_iter().enumerate() {
        let weak = weak.clone();
        let healthy = items.iter().filter(|(_, ok, _)| *ok).count();
        let mut cell = v()
            .id(("proxy-tools-face", index))
            .px(px(18.))
            .py(px(14.))
            .border_b(px(1.))
            .border_color(line())
            .cursor_pointer()
            .bg(if state.face == index {
                surface_2()
            } else {
                black()
            })
            .hover(|this| this.bg(surface_2()))
            .on_click(move |_, _, cx| {
                let _ = weak.update(cx, |app, cx| app.select_proxy_face(index, cx));
            })
            .child(
                h().items_center()
                    .justify_between()
                    .gap(px(12.))
                    .child(txt(title, 13., FontWeight::NORMAL, text()))
                    .child(caps(
                        &format!("{healthy} / {}", items.len()),
                        9.5,
                        FontWeight::NORMAL,
                        if healthy == items.len() && healthy > 0 {
                            ok()
                        } else {
                            dim()
                        },
                        0.1,
                    )),
            )
            .child(txt(description, 11.5, FontWeight::NORMAL, dim()).mt(px(9.)));
        let mut list = v().mt(px(12.)).gap(px(6.));
        for (name, good, detail) in items {
            list = list.child(
                h().items_start()
                    .gap(px(8.))
                    .child(div().mt(px(5.)).size(px(5.)).rounded_full().bg(if good {
                        ok()
                    } else {
                        accent()
                    }))
                    .child(
                        v().min_w_0()
                            .child(txt(name, 10.5, FontWeight::NORMAL, muted()))
                            .child(txt(detail, 9., FontWeight::NORMAL, dim())),
                    ),
            );
        }
        cell = cell.child(list);
        if index == last {
            cell = cell.border_b(px(0.));
        }
        column = column.child(cell);
    }
    column
}

fn output_side(overview: &Overview, weak: &WeakEntity<App>) -> Div {
    let state = overview.proxy;
    let mut tabs = h()
        .h(px(44.))
        .flex_shrink_0()
        .items_stretch()
        .border_b(px(1.))
        .border_color(line());
    for tab in Tab::ALL {
        let weak = weak.clone();
        tabs = tabs.child(
            div()
                .id(("proxy-tools-tab", tab as usize))
                .cursor_pointer()
                .px(px(20.))
                .flex()
                .items_center()
                .border_r(px(1.))
                .border_color(line())
                .when(state.tab == tab, |this| {
                    this.border_b(px(1.)).border_color(accent())
                })
                .hover(|this| this.bg(alpha(0xffffff06)))
                .on_click(move |_, _, cx| {
                    let _ = weak.update(cx, |app, cx| app.select_proxy_tab(tab, cx));
                })
                .child(caps(
                    tab.label(),
                    10.,
                    FontWeight::NORMAL,
                    if state.tab == tab { text() } else { dim() },
                    0.12,
                )),
        );
    }
    let outcome = state.outcome(state.tab);
    let text_color = if outcome.is_some_and(|outcome| !outcome.ok) {
        warn()
    } else if state.busy {
        warn()
    } else {
        text()
    };
    v().flex_1()
        .min_w_0()
        .child(tabs)
        .child(
            v().id("proxy-tools-output")
                .flex_1()
                .min_h_0()
                .p(px(20.))
                .overflow_scroll()
                .bg(black())
                .font_family("Noto Sans Mono CJK SC")
                .text_size(px(11.))
                .line_height(px(20.))
                .text_color(text_color)
                .child(verbatim_output(outcome, state.busy)),
        )
        .child(conclusion_bar(overview))
}

fn verbatim_output(outcome: Option<&proxy_tools::Outcome>, busy: bool) -> Div {
    let mut panel = v().gap(px(0.));
    if let Some(outcome) = outcome {
        panel = panel.child(
            div()
                .text_color(accent())
                .child(format!("$ {}", outcome.command)),
        );
        for line in outcome.output.lines() {
            let trimmed = line.trim_start();
            let color = if trimmed.starts_with('✗') {
                accent()
            } else if trimmed.starts_with('✓') {
                ok()
            } else if trimmed.starts_with('#') || trimmed.starts_with('$') {
                warn()
            } else if line.contains("绝不执行") || line.contains("受保护端口") {
                warn()
            } else if trimmed.is_empty() {
                dim()
            } else {
                muted()
            };
            panel = panel.child(div().text_color(color).child(line.to_string()));
        }
    } else if busy {
        panel = panel.child(
            div()
                .text_color(warn())
                .child("执行中… 等待内建后端返回结构化结果"),
        );
    } else {
        panel = panel.child(
            div()
                .text_color(dim())
                .child("点「STATUS」读取真实中继状态；HANDOVER 只打印命令、不会执行"),
        );
    }
    panel
}

fn conclusion_bar(overview: &Overview) -> Div {
    let state = overview.proxy;
    let outcome = state.outcome(state.tab);
    let (label, color) = match outcome {
        Some(outcome) if !outcome.ok => ("FAILED · EXIT NON-ZERO", accent()),
        Some(_) => ("EXIT 0 · 内建真实状态", ok()),
        None => ("WAITING · 未执行", dim()),
    };
    h().h(px(52.))
        .flex_shrink_0()
        .items_center()
        .justify_between()
        .gap(px(20.))
        .px(px(20.))
        .border_t(px(1.))
        .border_color(line())
        .bg(surface_2())
        .child(txt(
            if state.busy {
                "执行中…".to_string()
            } else {
                "只读 op 不会改动任何系统状态".to_string()
            },
            11.5,
            FontWeight::NORMAL,
            muted(),
        ))
        .child(pill(label, color))
}

fn pill(label: &str, color: Hsla) -> Div {
    div()
        .flex_shrink_0()
        .border(px(1.))
        .border_color(line())
        .px(px(10.))
        .py(px(5.))
        .child(caps(label, 10., FontWeight::NORMAL, color, 0.1))
}

fn fields(overview: &Overview) -> Div {
    let facts = overview.proxy.facts.clone().unwrap_or_default();
    v().child(field(
        "配置",
        &format!(
            "{} · listen {} · upstream {} · {}s 空闲 · {} 次重试",
            facts.paths.config,
            facts.config.listen,
            facts.config.upstream,
            facts.config.idle_timeout_secs,
            facts.config.retries
        ),
        "apply 写入",
        false,
    ))
    .child(field(
        "单元",
        &format!(
            "{} · ExecStart 指向 GNOUS 自身 --builtin-proxy-relay；旧 j1900-proxy.service 只读",
            facts.paths.unit
        ),
        "只启停自己",
        false,
    ))
    .child(field(
        "交棒",
        &format!(
            "把 {} 从旧中继交给 GNOUS 会中断所有在途连接（含本次对话），因此只打印命令，GNOUS 永不自动执行",
            facts.config.listen
        ),
        "手工人手执行",
        true,
    ))
}

fn field(label: &str, value: &str, hint: &'static str, danger: bool) -> Div {
    h().items_stretch()
        .border(px(1.))
        .border_color(line())
        .border_t(px(0.))
        .child(
            div()
                .w(px(120.))
                .flex_shrink_0()
                .p(px(14.))
                .border_r(px(1.))
                .border_color(line())
                .child(caps(label, 10., FontWeight::NORMAL, dim(), 0.18)),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .p(px(14.))
                .font_family("Noto Sans Mono CJK SC")
                .text_size(px(11.))
                .text_color(muted())
                .child(value.to_string()),
        )
        .child(
            div()
                .p(px(10.))
                .flex_shrink_0()
                .flex()
                .items_center()
                .border_l(px(1.))
                .border_color(line())
                .child(
                    div()
                        .px(px(14.))
                        .py(px(8.))
                        .border(px(1.))
                        .border_color(if danger { accent() } else { line_strong() })
                        .child(caps(
                            hint,
                            9.5,
                            FontWeight::NORMAL,
                            if danger { accent() } else { muted() },
                            0.1,
                        )),
                ),
        )
}

fn footer() -> Div {
    h().w_full()
        .items_start()
        .justify_between()
        .gap(px(30.))
        .border_t(px(1.))
        .border_color(line())
        .mt(px(44.))
        .pt(px(24.))
        .child(
            txt(
                "本页只读探测真实端口 / 单元 / 环境；GNOUS 的写操作仅限 ~/.config/gnous/proxy-tools.json 与自己的 user 单元文件，不启停、不改写承载本机联网的旧中继，也不动 gsettings 与 environment.d。交棒命令只打印，由人手在合适时刻执行。",
                12.,
                FontWeight::NORMAL,
                dim(),
            )
            .line_height(px(22.)),
        )
        .child(pill("READ-ONLY · 不接管受保护端口", ok()))
}

/// Keeps the page viewport stable while stdout is long.
pub const CONSOLE_HEIGHT: f32 = 620.;
