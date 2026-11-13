//! Renderer for the `root-align` page-level extension (demo C · 诊断台).
//!
//! Mirrors `demo/root-align/c.html`: a real conclusion strip, four functional
//! domains on the left, and verbatim `status` / `doctor` / action stdout on the
//! right. All state is owned by [`crate::root_align::State`].

use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    App as GpuiApp, Div, FontWeight, Hsla, InteractiveElement as _, ParentElement as _, Stateful,
    StatefulInteractiveElement as _, Styled, WeakEntity, div, px,
};

use crate::root_align::{self, Action, BUILTIN_STATE_FILE, DOMAINS, Status, Tab, UNITS, Unit};
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
        .child(fields(overview, weak))
        .child(footer())
}

fn status_color(status: Status) -> Hsla {
    match status {
        Status::Aligned => ok(),
        Status::Attention => warn(),
        Status::Failed => accent(),
        Status::Unknown => muted(),
    }
}

fn hero(overview: &Overview, weak: &WeakEntity<App>) -> Div {
    let state = overview.root_align;
    let mut actions = h().items_center().gap(px(8.));
    for action in Action::ALL
        .into_iter()
        .filter(|action| *action != Action::RestartShell)
    {
        let weak_action = weak.clone();
        let armed = state.armed == Some(action);
        let label = if armed {
            match action {
                Action::Apply => "确认 APPLY",
                Action::Revert => "确认 REVERT",
                _ => action.label(),
            }
        } else {
            action.label()
        };
        actions = actions.child(button(
            label,
            action == Action::Apply || armed,
            action == Action::Revert || armed,
            state.busy,
            move |cx| {
                let _ = weak_action.update(cx, |app, cx| app.run_root_align(action, cx));
            },
        ));
    }
    v().mb(px(44.))
        .child(
            caps(
                "自研扩展 · root-align · v0.1.0",
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
                            txt("Root 桌面对齐", 44., FontWeight::EXTRA_LIGHT, text())
                                .line_height(px(46.)),
                        )
                        .child(
                            txt(
                                "诊断台：左侧按功能域列出受影响能力，右侧原样展示内建后端 stdout；状态、doctor 日志、动作输出互不遮掩。",
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
    let id = format!("root-align-action-{label}");
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
    let state = overview.root_align;
    let healthy_count = state.units.iter().filter(|unit| unit.healthy).count();
    let values = [
        (
            "对齐结论",
            state.conclusion.label().to_string(),
            String::new(),
            status_color(state.conclusion),
        ),
        (
            "覆盖 / 活动",
            healthy_count.to_string(),
            format!("/ {}", UNITS.len()),
            text(),
        ),
        ("系统文件改动", "00".to_string(), "处".to_string(), text()),
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
                                if index == 0 { 30. } else { 44. },
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

fn console(overview: &Overview, weak: &WeakEntity<App>) -> Div {
    h().w_full()
        .items_stretch()
        .h(px(CONSOLE_HEIGHT))
        .border(px(1.))
        .border_color(line())
        .child(domains(overview, weak))
        .child(output_side(overview, weak))
}

fn domains(overview: &Overview, weak: &WeakEntity<App>) -> Div {
    let state = overview.root_align;
    let mut column = v()
        .w(px(330.))
        .flex_shrink_0()
        .border_r(px(1.))
        .border_color(line());
    for (index, domain) in DOMAINS.iter().enumerate() {
        let units: Vec<&Unit> = UNITS.iter().filter(|unit| unit.domain == index).collect();
        let healthy = units
            .iter()
            .filter(|unit| state.unit(unit.name).is_some_and(|parsed| parsed.healthy))
            .count();
        let weak = weak.clone();
        let mut cell = v()
            .id(("root-align-domain", index))
            .p(px(18.))
            .border_b(px(1.))
            .border_color(line())
            .cursor_pointer()
            .bg(if state.domain == index {
                surface_2()
            } else {
                black()
            })
            .hover(|this| this.bg(surface_2()))
            .on_click(move |_, _, cx| {
                let _ = weak.update(cx, |app, cx| app.select_root_domain(index, cx));
            })
            .child(
                h().items_center()
                    .justify_between()
                    .gap(px(12.))
                    .child(txt(domain.name, 13., FontWeight::NORMAL, text()))
                    .child(caps(
                        &format!("{healthy} / {}", units.len()),
                        9.5,
                        FontWeight::NORMAL,
                        if healthy == units.len() && healthy > 0 {
                            ok()
                        } else {
                            dim()
                        },
                        0.1,
                    )),
            )
            .child(txt(domain.description, 11.5, FontWeight::NORMAL, dim()).mt(px(9.)));
        let mut list = v().mt(px(12.)).gap(px(2.));
        for unit in units {
            let parsed = state.unit(unit.name);
            let color = match parsed {
                Some(value) if value.healthy => ok(),
                Some(_) => accent(),
                None => dim(),
            };
            list = list.child(
                h().items_center()
                    .gap(px(8.))
                    .child(div().size(px(5.)).rounded_full().bg(color))
                    .child(txt(unit.name, 10.5, FontWeight::NORMAL, muted()))
                    .child(txt(unit.description, 9., FontWeight::NORMAL, dim())),
            );
        }
        cell = cell.child(list);
        if index == DOMAINS.len() - 1 {
            cell = cell.border_b(px(0.));
        }
        column = column.child(cell);
    }
    column
}

fn output_side(overview: &Overview, weak: &WeakEntity<App>) -> Div {
    let state = overview.root_align;
    let mut tabs = h()
        .h(px(44.))
        .flex_shrink_0()
        .items_stretch()
        .border_b(px(1.))
        .border_color(line());
    for tab in [Tab::Status, Tab::Doctor, Tab::Action] {
        let weak = weak.clone();
        tabs = tabs.child(
            div()
                .id(("root-align-tab", tab as usize))
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
                    let _ = weak.update(cx, |app, cx| app.select_root_tab(tab, cx));
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
            v().id("root-align-output")
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
        .child(conclusion_bar(state))
}

/// GPUI text nodes preserve newlines; each stdout line is its own child so long
/// output stays readable and the panel scrolls instead of stretching the page.
fn verbatim_output(outcome: Option<&root_align::Outcome>, busy: bool) -> Div {
    let mut panel = v().gap(px(0.));
    if let Some(outcome) = outcome {
        panel = panel.child(
            div()
                .text_color(accent())
                .child(format!("$ {}", outcome.command)),
        );
        for line in outcome.output.lines() {
            let trimmed = line.trim_start();
            let spaces = line.chars().take_while(|ch| ch.is_whitespace()).count();
            if trimmed.starts_with('✓') || trimmed.starts_with('!') || trimmed.starts_with('✗')
            {
                let marker = &trimmed[..trimmed.chars().next().unwrap().len_utf8()];
                let rest = &trimmed[marker.len()..];
                let marker_color = if marker == "✓" { ok() } else { warn() };
                let rest_color = if line.contains("SUSPENDED") {
                    warn()
                } else {
                    muted()
                };
                panel = panel.child(
                    h().pl(px(spaces as f32 * 5.5))
                        .child(div().text_color(marker_color).child(marker.to_string()))
                        .child(div().text_color(rest_color).child(rest.to_string())),
                );
            } else {
                let color = if line.contains("已完全对齐") {
                    ok()
                } else if line.contains("ConditionUser") || line.contains("SUSPENDED") {
                    warn()
                } else if line.contains("──") || spaces >= 6 {
                    dim()
                } else {
                    muted()
                };
                panel = panel.child(div().text_color(color).child(line.to_string()));
            }
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
                .child("点「STATUS」或「DOCTOR」读取 GNOUS 内建后端的真实状态"),
        );
    }
    panel
}

fn conclusion_bar(state: &root_align::State) -> Div {
    let outcome = state.outcome(state.tab);
    let (label, color) = match outcome {
        Some(outcome) if !outcome.ok => ("FAILED · EXIT NON-ZERO", accent()),
        Some(outcome) => (
            match outcome.action {
                Action::Status => {
                    if state.conclusion == Status::Aligned {
                        "STATUS · 已完全对齐"
                    } else {
                        state.conclusion.label()
                    }
                }
                Action::Doctor => "DOCTOR · EXIT 0",
                Action::Apply | Action::Revert | Action::RestartShell => "ACTION · EXIT 0",
            },
            ok(),
        ),
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
            format!(
                "{} · {} · {}",
                label,
                state.checked,
                if state.busy { "执行中" } else { "已就绪" }
            ),
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

fn fields(overview: &Overview, weak: &WeakEntity<App>) -> Div {
    let state = overview.root_align;
    v().child(field(
        "后端目录",
        &root_align::fixes_backend::backend_dir().to_string_lossy(),
        "GNOUS_STATE_HOME 覆盖",
        false,
        None,
    ))
    .child(field(
        "状态文件",
        &format!("{BUILTIN_STATE_FILE} · JSON 记录 apply 写过的覆盖，供 revert 精确回滚"),
        "只读展示",
        false,
        None,
    ))
    .child(field(
        "危险动作",
        "restart-shell 会打断当前 GNOME 会话，默认关闭，UI 不从批量流程触发",
        if state.armed == Some(Action::RestartShell) {
            "确认重启 SHELL"
        } else {
            "二次确认"
        },
        true,
        Some(weak.clone()),
    ))
}

fn field(
    label: &str,
    value: &str,
    button_label: &'static str,
    danger: bool,
    weak: Option<WeakEntity<App>>,
) -> Div {
    let field_id = format!("root-align-field-{label}");
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
                        .id(field_id)
                        .when(weak.is_some(), |this| this.cursor_pointer())
                        .px(px(14.))
                        .py(px(8.))
                        .border(px(1.))
                        .border_color(if danger { accent() } else { line_strong() })
                        .on_click(move |_, _, cx| {
                            if let Some(weak) = &weak {
                                let _ = weak.update(cx, |app, cx| {
                                    app.run_root_align(Action::RestartShell, cx)
                                });
                            }
                        })
                        .child(caps(
                            button_label,
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
                "绿字来自 GNOUS 内建后端的结构化事实；命令失败或事实无法解析时显示 UNKNOWN / FAILED 与原因。只在 root 的 ~/.config/systemd/user 写覆盖，不改 /usr/lib，不 enable/disable 全局单元，joe 会话不受影响。",
                12.,
                FontWeight::NORMAL,
                dim(),
            )
            .line_height(px(22.)),
        )
        .child(pill("STATUS · 内建真实状态", ok()))
}

/// Used by `App` to keep the page viewport stable while stdout is long.
pub const CONSOLE_HEIGHT: f32 = 480.;
