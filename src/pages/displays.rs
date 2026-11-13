//! Renderer for the `displays`（多屏幕）page-level extension.
//!
//! Same shape as the other diagnostic pages: a conclusion strip driven by real
//! facts, three faces on the left, verbatim backend stdout on the right. The
//! panel never guesses: if mutter can't be reached, it says so and shows the
//! sysfs truth instead of inventing a layout.

use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    App as GpuiApp, Div, FontWeight, Hsla, InteractiveElement as _, ParentElement as _, Stateful,
    StatefulInteractiveElement as _, Styled, WeakEntity, div, px,
};

use crate::displays::{self, Op, StatusFacts, Tab};
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

fn headline_color(facts: Option<&StatusFacts>) -> Hsla {
    match facts.map(|facts| facts.headline.as_str()) {
        Some("healthy") => ok(),
        Some("attention") => warn(),
        Some("degraded") => accent(),
        _ => muted(),
    }
}

fn hero(overview: &Overview, weak: &WeakEntity<App>) -> Div {
    let state = overview.displays;
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
            op == Op::Apply || op == Op::Revert,
            state.busy,
            move |cx| {
                let _ = weak.update(cx, |app, cx| app.run_displays(op, cx));
            },
        ));
    }
    v().mb(px(44.))
        .child(
            caps(
                "自研扩展 · displays · v0.1.0",
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
                            txt("多屏幕", 44., FontWeight::EXTRA_LIGHT, text())
                                .line_height(px(46.)),
                        )
                        .child(
                            txt(
                                "接管外接屏 / 内置屏的布局：状态来自 mutter 的真实应答，APPLY 先在 verify 模式下校验再落地，老脚本的 timer 只读。",
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
        (line_strong(), black(), muted())
    } else {
        (line_strong(), black(), muted())
    };
    let id = format!("displays-action-{label}");
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
    let facts = overview.displays.facts.clone();
    let checks_ok = facts
        .as_ref()
        .map(|facts| facts.checks.iter().filter(|check| check.ok).count())
        .unwrap_or(0);
    let checks_total = facts.as_ref().map(|facts| facts.checks.len()).unwrap_or(0);
    let active = facts
        .as_ref()
        .map(|facts| facts.active.join("+"))
        .filter(|text| !text.is_empty())
        .unwrap_or_else(|| "—".to_string());
    let desired = facts
        .as_ref()
        .map(|facts| facts.desired.join("+"))
        .filter(|text| !text.is_empty())
        .unwrap_or_else(|| "—".to_string());
    let values = [
        (
            "显示结论",
            facts
                .as_ref()
                .map(|facts| facts.headline.to_uppercase())
                .filter(|text| !text.is_empty())
                .unwrap_or_else(|| "—".to_string()),
            String::new(),
            headline_color(facts.as_ref()),
            26.,
        ),
        ("当前布局", active, String::new(), text(), 40.),
        ("期望布局", desired, String::new(), text(), 40.),
        (
            "诊断项",
            format!("{checks_ok}"),
            format!("/ {checks_total}"),
            text(),
            44.,
        ),
    ];
    let mut row = h()
        .w_full()
        .items_stretch()
        .border_t(px(1.))
        .border_b(px(1.))
        .border_color(line());
    for (index, (label, value, unit, color, size)) in values.into_iter().enumerate() {
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
                            txt(value, size, FontWeight::EXTRA_LIGHT, color).line_height(px(46.)),
                        )
                        .when(!unit.is_empty(), |this| {
                            this.child(txt(unit, 13., FontWeight::LIGHT, muted()))
                        }),
                ),
        );
    }
    row
}

/// 一个面 = (标题, 说明, [(条目, 是否正常, 细节)])。
fn faces(facts: &StatusFacts) -> Vec<(&'static str, &'static str, Vec<(String, bool, String)>)> {
    let connectors = facts
        .connectors
        .iter()
        .map(|c| format!("{}={}", c.name, c.sysfs))
        .collect::<Vec<_>>()
        .join(", ");
    vec![
        (
            "诊断 · 只读",
            "sysfs、mutter 状态、老脚本单元；不改任何东西。",
            vec![
                (
                    "会话总线".to_string(),
                    facts.bus_ok,
                    match &facts.bus_error {
                        Some(error) => error.clone(),
                        None => format!("mutter 序号 {}", facts.serial),
                    },
                ),
                (
                    "物理接口".to_string(),
                    !facts.connectors.is_empty(),
                    if connectors.is_empty() {
                        "没有任何 DRM 接口".to_string()
                    } else {
                        connectors
                    },
                ),
                ("老脚本".to_string(), true, facts.legacy.line.clone()),
            ],
        ),
        (
            "布局 · mutter",
            "APPLY 先 verify 再持久化；PREVIEW 只校验不生效。",
            vec![
                (
                    "当前".to_string(),
                    !facts.active.is_empty(),
                    if facts.active.is_empty() {
                        "没有点亮的接口".to_string()
                    } else {
                        facts.active.join("+")
                    },
                ),
                (
                    "期望".to_string(),
                    !facts.desired.is_empty(),
                    format!("{} → {}", facts.config.mode, facts.desired.join("+")),
                ),
                (
                    "一致性".to_string(),
                    !facts.drift,
                    if facts.drift {
                        "实际与期望不一致".to_string()
                    } else {
                        "实际等于期望".to_string()
                    },
                ),
            ],
        ),
        (
            "接管 · GNOUS",
            "只写 gnous-display.*，从不改老脚本的单元。",
            vec![
                (
                    "GNOUS 单元".to_string(),
                    facts.own.present,
                    if facts.own.present {
                        facts.own.line.clone()
                    } else {
                        "未安装（APPLY 只写文件）".to_string()
                    },
                ),
                (
                    "配置".to_string(),
                    facts.config.warning.is_none(),
                    format!(
                        "{} · 对账 {}s",
                        facts.config.path, facts.config.reconcile_secs
                    ),
                ),
                (
                    "交棒".to_string(),
                    facts.config.takeover,
                    if facts.config.takeover {
                        "显示已由 GNOUS 管".to_string()
                    } else {
                        "仍由老脚本对账（HANDOVER 只打印命令）".to_string()
                    },
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
    let state = overview.displays;
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
            .id(("displays-face", index))
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
                let _ = weak.update(cx, |app, cx| app.select_displays_face(index, cx));
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
    let state = overview.displays;
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
                .id(("displays-tab", tab as usize))
                .cursor_pointer()
                .px(px(18.))
                .flex()
                .items_center()
                .border_r(px(1.))
                .border_color(line())
                .when(state.tab == tab, |this| {
                    this.border_b(px(1.)).border_color(accent())
                })
                .hover(|this| this.bg(alpha(0xffffff06)))
                .on_click(move |_, _, cx| {
                    let _ = weak.update(cx, |app, cx| app.select_displays_tab(tab, cx));
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
            v().id("displays-output")
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

fn verbatim_output(outcome: Option<&displays::Outcome>, busy: bool) -> Div {
    let mut panel = v().gap(px(0.));
    if let Some(outcome) = outcome {
        for line in outcome.render().lines() {
            let trimmed = line.trim_start();
            let color = if trimmed.starts_with('✗') {
                accent()
            } else if trimmed.starts_with('✓') {
                ok()
            } else if trimmed.starts_with('#') || trimmed.starts_with('$') {
                warn()
            } else if line.contains("未改动显示") || line.contains("没有执行") {
                warn()
            } else if trimmed.starts_with("[dry-run]") {
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
                .child("执行中… 等待 D-Bus / systemd 返回"),
        );
    } else {
        panel = panel.child(
            div()
                .text_color(dim())
                .child("点「STATUS」读真实布局；PREVIEW 只校验，APPLY 需要二次确认"),
        );
    }
    panel
}

fn conclusion_bar(overview: &Overview) -> Div {
    let state = overview.displays;
    let outcome = state.outcome(state.tab);
    let (label, color) = match outcome {
        Some(outcome) if !outcome.ok => ("FAILED · EXIT NON-ZERO", accent()),
        Some(outcome) if outcome.changed => ("EXIT 0 · 已改变显示", warn()),
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
                "只有 APPLY 会改显示；PREVIEW / STATUS / DOCTOR 都不会".to_string()
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
    let facts = overview.displays.facts.clone().unwrap_or_default();
    let monitors = facts
        .monitors
        .iter()
        .map(|m| {
            format!(
                "{} {} 当前={} 首选={} {} 个模式",
                m.connector,
                m.product,
                m.current.clone().unwrap_or_else(|| "-".into()),
                m.preferred.clone().unwrap_or_else(|| "-".into()),
                m.mode_count
            )
        })
        .collect::<Vec<_>>()
        .join(" · ");
    v().child(field(
        "配置",
        &format!(
            "{} · mode {} · {}→{} · 分辨率 {} · 对账 {}s",
            facts.config.path,
            facts.config.mode,
            facts.config.internal,
            facts.config.external,
            facts.config.resolution,
            facts.config.reconcile_secs
        ),
        "apply 写入",
        false,
    ))
    .child(field(
        "显示器",
        if monitors.is_empty() {
            "mutter 没有报告任何显示器"
        } else {
            &monitors
        },
        "来自 mutter",
        false,
    ))
    .child(field(
        "交棒",
        &format!(
            "把对账从 {} 换成 GNOUS 自己的 timer 需要人手执行 HANDOVER 里的命令；GNOUS 只写 gnous-display.*，从不启停老脚本。",
            facts.legacy.line
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
                "状态取自 /sys/class/drm 与 mutter 的 GetCurrentState；APPLY 走 ApplyMonitorsConfig，先 METHOD_VERIFY 校验、再 METHOD_PERSISTENT 落地，落地后重新读一次确认。GNOUS 只写 ~/.config/gnous/displays.json 与自己的 gnous-display.* 单元文件，从不启停或改写老脚本的 display-mode.timer。",
                12.,
                FontWeight::NORMAL,
                dim(),
            )
            .line_height(px(22.)),
        )
        .child(pill("只改显示 · 老 timer 只读", ok()))
}

/// Keeps the page viewport stable while stdout is long.
pub const CONSOLE_HEIGHT: f32 = 620.;
