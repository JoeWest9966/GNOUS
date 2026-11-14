//! Renderer for the `termfont`（终端字体）page-level extension.
//!
//! Same shape as the other diagnostic pages: a conclusion strip driven by real
//! facts, three faces on the left, verbatim backend stdout on the right. The
//! only addition is a pair of steppers at the bottom: they edit the *target*
//! numbers (a JSON file), never the machine — the machine only changes when
//! TERM / FS / ALL is confirmed.

use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    App as GpuiApp, Div, FontWeight, Hsla, InteractiveElement as _, ParentElement as _, Stateful,
    StatefulInteractiveElement as _, Styled, WeakEntity, div, px,
};

use crate::termfont::{self, Op, StatusFacts, Tab};
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
        .child(targets(overview, weak))
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
    let state = overview.termfont;
    let targets = state.facts.as_ref().map(|facts| facts.targets.clone());
    let mut actions = h().items_center().gap(px(6.));
    for op in Op::ALL {
        let weak = weak.clone();
        let armed = state.armed == Some(op);
        let label = if armed {
            match op {
                Op::Term => "确认 TERM",
                Op::Fs => "确认 FS",
                Op::All => "确认 ALL",
                Op::Follow => "确认 FOLLOW",
                Op::Restart => "确认 RESTART",
                _ => op.label(),
            }
        } else {
            op.label()
        };
        let solid = op == Op::Status || armed;
        let danger = op == Op::Restart;
        actions = actions.child(button(
            label,
            op.id(),
            solid,
            danger,
            state.busy,
            move |cx| {
                let _ = weak.update(cx, |app, cx| app.run_termfont(op, cx));
            },
        ));
    }
    let _ = targets;
    v().mb(px(44.))
        .child(
            caps(
                "自研扩展 · termfont · v0.1.0",
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
                            txt("终端字体", 44., FontWeight::EXTRA_LIGHT, text())
                                .line_height(px(46.)),
                        )
                        .child(
                            txt(
                                "GNOME 终端按 pt、FinalShell 按 px：读的是两者真实的当前值，改前先备份，改完重新读一次再报结论。",
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
    key: &'static str,
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
    let id = format!("termfont-action-{key}");
    div()
        .id(id)
        .cursor_pointer()
        .px(px(14.))
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
        .child(caps(label, 9.5, FontWeight::MEDIUM, fg, 0.16))
        .on_click(move |_, _, cx| {
            if !disabled {
                handler(cx);
            }
        })
}

fn metrics(overview: &Overview) -> Div {
    let facts = overview.termfont.facts.clone();
    let checks_ok = facts
        .as_ref()
        .map(|facts| facts.checks.iter().filter(|check| check.ok).count())
        .unwrap_or(0);
    let checks_total = facts.as_ref().map(|facts| facts.checks.len()).unwrap_or(0);
    let term = facts
        .as_ref()
        .and_then(|facts| facts.term.effective_pt)
        .map(termfont::config::fmt_num)
        .unwrap_or_else(|| "—".to_string());
    let fs = facts
        .as_ref()
        .and_then(|facts| facts.fs.px)
        .map(|px| px.to_string())
        .unwrap_or_else(|| "—".to_string());
    let values = [
        (
            "字号结论",
            facts
                .as_ref()
                .map(|facts| facts.headline.to_uppercase())
                .filter(|text| !text.is_empty())
                .unwrap_or_else(|| "—".to_string()),
            String::new(),
            headline_color(facts.as_ref()),
            24.,
        ),
        ("GNOME 终端", term, "pt".to_string(), text(), 40.),
        ("FinalShell", fs, "px".to_string(), text(), 40.),
        (
            "自检",
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
    let term_detail = match &facts.term.error {
        Some(error) => error.clone(),
        None => format!(
            "{}{}",
            facts.term.raw,
            if facts.term.use_system {
                "（跟随系统，没生效）"
            } else {
                ""
            }
        ),
    };
    vec![
        (
            "诊断 · 只读",
            "gsettings 与 FinalShell 配置；不改任何东西。",
            vec![
                (
                    "系统等宽".to_string(),
                    facts.system_error.is_none(),
                    match &facts.system_error {
                        Some(error) => error.clone(),
                        None => facts.system_raw.clone(),
                    },
                ),
                ("终端 profile".to_string(), true, facts.term.profile.clone()),
                (
                    "终端字号".to_string(),
                    facts.term.error.is_none(),
                    term_detail,
                ),
            ],
        ),
        (
            "字号 · 真实值",
            "终端单位 pt，FinalShell 单位 px；换算 1 pt = 4/3 px。",
            vec![
                (
                    "GNOME 终端".to_string(),
                    facts.term_matches,
                    format!(
                        "{} {} pt",
                        facts.term.effective_family,
                        facts
                            .term
                            .effective_pt
                            .map(termfont::config::fmt_num)
                            .unwrap_or_else(|| "?".into())
                    ),
                ),
                (
                    "FinalShell".to_string(),
                    facts.fs_matches,
                    match facts.fs.px {
                        Some(px) => format!(
                            "{} px ≈ {} pt · {}",
                            px,
                            termfont::config::fmt_num(termfont::config::px2pt(px)),
                            if facts.fs.running {
                                format!("运行中 {} 个进程", facts.fs.pids)
                            } else {
                                "未运行".to_string()
                            }
                        ),
                        None => "读不到配置".to_string(),
                    },
                ),
                (
                    "一致性".to_string(),
                    facts.term_matches && facts.fs_matches,
                    if facts.term_matches && facts.fs_matches {
                        "两者都等于目标".to_string()
                    } else {
                        facts.headline_reason.clone()
                    },
                ),
            ],
        ),
        (
            "目标 · GNOUS",
            "只改目标值不会动机器；要生效得点 TERM / FS / ALL。",
            vec![
                (
                    "目标".to_string(),
                    true,
                    format!(
                        "{} pt / {} px · 换算 {}",
                        termfont::config::fmt_num(facts.targets.term_pt),
                        facts.targets.convert_px,
                        if facts.targets.auto_convert {
                            "开"
                        } else {
                            "关"
                        }
                    ),
                ),
                (
                    "配置".to_string(),
                    facts.targets.warning.is_none(),
                    match &facts.targets.warning {
                        Some(warning) => warning.clone(),
                        None => facts.targets.path.clone(),
                    },
                ),
                (
                    "回滚".to_string(),
                    facts.fs.backup_present,
                    if facts.fs.backup_present {
                        facts.fs.backup.clone()
                    } else {
                        "第一次改 FinalShell 时自动备份".to_string()
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
    let state = overview.termfont;
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
            .id(("termfont-face", index))
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
                let _ = weak.update(cx, |app, cx| app.select_termfont_face(index, cx));
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
    let state = overview.termfont;
    let mut tabs = h()
        .h(px(44.))
        .flex_shrink_0()
        .items_stretch()
        .border_b(px(1.))
        .border_color(line());
    for tab in Tab::ALL {
        let weak = weak.clone();
        let danger = tab.op().is_dangerous();
        tabs = tabs.child(
            div()
                .id(("termfont-tab", tab as usize))
                .cursor_pointer()
                .px(px(16.))
                .flex()
                .items_center()
                .border_r(px(1.))
                .border_color(line())
                .when(state.tab == tab, |this| {
                    this.border_b(px(1.)).border_color(accent())
                })
                .hover(|this| this.bg(alpha(0xffffff06)))
                .on_click(move |_, _, cx| {
                    let _ = weak.update(cx, |app, cx| app.select_termfont_tab(tab, cx));
                })
                .child(caps(
                    tab.label(),
                    10.,
                    FontWeight::NORMAL,
                    if state.tab == tab {
                        if danger { accent() } else { text() }
                    } else if danger {
                        accent()
                    } else {
                        dim()
                    },
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
            v().id("termfont-output")
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

fn verbatim_output(outcome: Option<&termfont::Outcome>, busy: bool) -> Div {
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
            } else if line.contains("✗") || line.contains("注意") || line.contains("断开") {
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
        panel = panel.child(div().text_color(warn()).child("执行中…"));
    } else {
        panel = panel.child(
            div()
                .text_color(dim())
                .child("点「STATUS」读真实字号；TERM / FS / ALL 需要二次确认，RESTART 会断 SSH"),
        );
    }
    panel
}

fn conclusion_bar(overview: &Overview) -> Div {
    let state = overview.termfont;
    let outcome = state.outcome(state.tab);
    let (label, color) = match outcome {
        Some(outcome) if !outcome.ok => ("FAILED · 有 ✗", accent()),
        Some(outcome) if outcome.changed => ("EXIT 0 · 已改字号", warn()),
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
            } else if let Some(facts) = &state.facts {
                let targets = termfont::config::Targets {
                    term_pt: facts.targets.term_pt,
                    fs_px: facts.targets.fs_px,
                    auto_convert: facts.targets.auto_convert,
                };
                {
                    let op = state.tab.op();
                    format!(
                        "{} · {}{}",
                        state.tab.label(),
                        op.describe(&targets),
                        if op.is_dangerous() {
                            "（会断 SSH，二次确认）"
                        } else if op.is_mutating() {
                            "（会改机器，二次确认）"
                        } else {
                            "（只读，不改机器）"
                        }
                    )
                }
            } else {
                "只有 TERM / FS / ALL / FOLLOW / RESTART 会改机器；RESTART 会断 SSH".to_string()
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

/// 目标值的步进器。它们只改 `~/.config/gnous/termfont.json`，不动机器。
fn targets(overview: &Overview, weak: &WeakEntity<App>) -> Div {
    let facts = overview.termfont.facts.clone().unwrap_or_default();
    let targets = &facts.targets;
    let term = termfont::config::fmt_num(targets.term_pt);
    let convert = if targets.auto_convert {
        format!("开（FinalShell 用换算值 {} px）", targets.convert_px)
    } else {
        format!("关（FinalShell 用单独的 {} px）", targets.fs_px)
    };
    v().child(stepper(
        "目标 · 终端",
        &term,
        "pt",
        "步进 0.5 pt",
        weak,
        |app, cx| app.step_termfont(-1, 0, cx),
        |app, cx| app.step_termfont(1, 0, cx),
    ))
    .child(stepper(
        "目标 · FinalShell",
        &targets.fs_px.to_string(),
        "px",
        "步进 1 px（会关掉换算）",
        weak,
        |app, cx| app.step_termfont(0, -1, cx),
        |app, cx| app.step_termfont(0, 1, cx),
    ))
    .child(row(
        "换算",
        &convert,
        "1 pt = 4/3 px",
        weak,
        Some(("切换", |app, cx| app.toggle_termfont_convert(cx))),
        false,
    ))
    .child(row(
        "配置",
        &format!(
            "{}{}{}",
            targets.path,
            match &targets.warning {
                Some(warning) => format!(" · {warning}"),
                None => String::new(),
            },
            match &overview.termfont.edit_error {
                Some(error) => format!(" · ✗ 写配置失败：{error}"),
                None => String::new(),
            }
        ),
        if overview.termfont.edit_error.is_some() {
            "✗ 写入失败"
        } else {
            "步进即写入"
        },
        weak,
        None,
        overview.termfont.edit_error.is_some(),
    ))
    .child(row(
        "回滚",
        &format!(
            "{} · {}",
            facts.fs.backup,
            if facts.fs.backup_present {
                "存在，可直接 cp 回去"
            } else {
                "还没有备份"
            }
        ),
        "改前自动备份",
        weak,
        None,
        true,
    ))
}

fn row(
    label: &str,
    value: &str,
    hint: &'static str,
    weak: &WeakEntity<App>,
    action: Option<(&'static str, fn(&mut App, &mut gpui_kit::Context<App>))>,
    danger: bool,
) -> Stateful<Div> {
    let mut cell = h()
        .id(format!("termfont-row-{label}"))
        .items_stretch()
        .border(px(1.))
        .border_color(line())
        .border_t(px(0.))
        .child(
            div()
                .w(px(160.))
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
                .text_color(if danger { dim() } else { muted() })
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
        );
    if let Some((_label, handler)) = action {
        let weak = weak.clone();
        cell = cell.cursor_pointer().on_click(move |_, _, cx| {
            let _ = weak.update(cx, |app, cx| handler(app, cx));
        });
    }
    cell
}

#[allow(clippy::too_many_arguments)]
fn stepper(
    label: &str,
    value: &str,
    unit: &str,
    hint: &'static str,
    weak: &WeakEntity<App>,
    minus: fn(&mut App, &mut gpui_kit::Context<App>),
    plus: fn(&mut App, &mut gpui_kit::Context<App>),
) -> Div {
    let weak_minus = weak.clone();
    let weak_plus = weak.clone();
    h().items_stretch()
        .border(px(1.))
        .border_color(line())
        .border_t(px(0.))
        .child(
            div()
                .w(px(160.))
                .flex_shrink_0()
                .p(px(14.))
                .border_r(px(1.))
                .border_color(line())
                .child(caps(label, 10., FontWeight::NORMAL, dim(), 0.18)),
        )
        .child(
            h().flex_1()
                .min_w_0()
                .items_center()
                .gap(px(12.))
                .p(px(8.))
                .child(step_button("−", move |cx| {
                    let _ = weak_minus.update(cx, |app, cx| minus(app, cx));
                }))
                .child(
                    h().items_end()
                        .gap(px(6.))
                        .child(txt(value, 20., FontWeight::EXTRA_LIGHT, text()))
                        .child(txt(unit, 11., FontWeight::NORMAL, muted())),
                )
                .child(step_button("+", move |cx| {
                    let _ = weak_plus.update(cx, |app, cx| plus(app, cx));
                })),
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
                        .border_color(line_strong())
                        .child(caps(hint, 9.5, FontWeight::NORMAL, muted(), 0.1)),
                ),
        )
}

fn step_button(label: &'static str, handler: impl Fn(&mut GpuiApp) + 'static) -> Stateful<Div> {
    let id = format!("termfont-step-{label}");
    div()
        .id(id)
        .cursor_pointer()
        .size(px(28.))
        .flex()
        .items_center()
        .justify_center()
        .border(px(1.))
        .border_color(line_strong())
        .child(txt(label, 14., FontWeight::LIGHT, muted()))
        .hover(|this| this.bg(alpha(0xffffff0a)))
        .on_click(move |_, _, cx| handler(cx))
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
            v().flex_1().min_w_0().child(txt(
                "状态取自 gsettings 的 org.gnome.Terminal.Legacy.Profile（默认 profile 的 font / use-system-font）与 ~/.finalshell/config.json 的 font_size。写终端之前只碰这两个键；写 FinalShell 之前备份成 config.json.bak，且只做定点文本替换（文件里的加密字段必须原样保留）。FinalShell 只在启动时读配置，所以改完要 RESTART 才生效——那一步会断开所有 SSH 会话。",
                12.,
                FontWeight::NORMAL,
                dim(),
            )
            .line_height(px(22.)),
        ))
        .child(pill("只改字号 · 写前备份", ok()))
}

/// Keeps the page viewport stable while stdout is long.
pub const CONSOLE_HEIGHT: f32 = 560.;
