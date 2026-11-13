//! Renderer for the `gnome-fixes` page-level extension.
//!
//! Mirrors `demo/gnome-fixes/index.html`: metric strip, five expandable rows,
//! evidence matrix + real command output on the left, explanation + command on
//! the right. All actions run the external tool through `crate::fixes`.

use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    Div, FontWeight, InteractiveElement as _, ParentElement as _, Stateful,
    StatefulInteractiveElement as _, Styled, WeakEntity, div, px, relative,
};

use crate::fixes::{self, Action, FIXES, Fix, Status};
use crate::theme::{
    accent, accent_hover, alpha, caps, dim, h, line, line_strong, muted, ok, surface_2, text, txt,
    v, warn,
};
use crate::{App, Overview};

/// Entry point from `content()` in `main.rs`.
pub fn body(overview: &Overview, weak: &WeakEntity<App>) -> Div {
    v().child(hero(overview, weak))
        .child(metrics(overview))
        .child(
            h().justify_between()
                .items_end()
                .mb(px(20.))
                .child(caps("修复项", 12., FontWeight::MEDIUM, muted(), 0.24))
                .child(caps("点击展开详情", 11., FontWeight::NORMAL, dim(), 0.12)),
        )
        .child(fix_list(overview, weak))
        .child(footer())
}

fn count(overview: &Overview, want: Status) -> usize {
    overview
        .fixes
        .iter()
        .filter(|state| state.status == want)
        .count()
}

fn hero(overview: &Overview, weak: &WeakEntity<App>) -> Div {
    let weak_all = weak.clone();
    let weak_apply = weak.clone();
    v().mb(px(44.))
        .child(
            caps(
                "自研扩展 · gnome-fixes · v0.1.0",
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
                            txt("系统修复", 44., FontWeight::EXTRA_LIGHT, text())
                                .line_height(px(46.)),
                        )
                        .child(
                            txt(
                                "把零散的小补丁收成一处：每项都走体检 → 应用 → 回退，随时可逆。",
                                13.,
                                FontWeight::NORMAL,
                                muted(),
                            )
                            .mt(px(12.)),
                        ),
                )
                .child(
                    h().items_center()
                        .gap(px(10.))
                        .flex_shrink_0()
                        .child(action_button("全部体检", false, move |cx| {
                            let _ = weak_all.update(cx, |app, cx| app.check_all_fixes(cx));
                        }))
                        .child(action_button(
                            if overview.fixes_apply_armed {
                                "确认应用"
                            } else {
                                "应用可用修复"
                            },
                            true,
                            move |cx| {
                                let _ =
                                    weak_apply.update(cx, |app, cx| app.apply_pending_fixes(cx));
                            },
                        )),
                ),
        )
}

fn action_button(
    label: &'static str,
    solid: bool,
    handler: impl Fn(&mut gpui_kit::App) + 'static,
) -> Stateful<Div> {
    div()
        .id(label)
        .cursor_pointer()
        .border(px(1.))
        .border_color(if solid { accent() } else { line_strong() })
        .when(solid, |this| this.bg(accent()))
        .hover(|this| {
            if solid {
                this.bg(accent_hover())
            } else {
                this.bg(alpha(0xffffff0a))
            }
        })
        .px(px(22.))
        .py(px(11.))
        .child(caps(label, 11., FontWeight::MEDIUM, text(), 0.16))
        .on_click(move |_, _, cx| handler(cx))
}

fn metrics(overview: &Overview) -> Div {
    let pending = count(overview, Status::Attention) + count(overview, Status::Unknown);
    let values = [
        ("修复项", FIXES.len().to_string(), "项"),
        ("已应用", count(overview, Status::Applied).to_string(), "项"),
        ("待处理", pending.to_string(), "项"),
        ("体检时间", overview.fixes_checked.to_string(), "今天"),
    ];
    let mut row = h()
        .w_full()
        .items_stretch()
        .border_t(px(1.))
        .border_b(px(1.))
        .border_color(line())
        .mb(px(44.));
    for (index, (label, value, unit)) in values.into_iter().enumerate() {
        row = row.child(
            v().flex_1()
                .min_w_0()
                .py(px(26.))
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
                                44.,
                                FontWeight::EXTRA_LIGHT,
                                if index == 2 && pending > 0 {
                                    accent()
                                } else {
                                    text()
                                },
                            )
                            .line_height(px(46.)),
                        )
                        .child(txt(unit, 13., FontWeight::LIGHT, muted())),
                ),
        );
    }
    row
}

fn fix_list(overview: &Overview, weak: &WeakEntity<App>) -> Div {
    let mut list = v().border(px(1.)).border_color(line()).mb(px(40.));
    for (index, fix) in FIXES.iter().enumerate() {
        let state = overview
            .fixes
            .iter()
            .find(|state| state.id == fix.id)
            .cloned()
            .unwrap_or_else(|| fixes::FixState::empty(fix.id));
        list = list.child(fix_row(index, fix, &state, weak));
    }
    list
}

/// One row. The whole row toggles, but each action button also toggles its own
/// state without opening the row (matches the demo's `e.target.tagName === 'U'`).
fn fix_row(index: usize, fix: &Fix, state: &fixes::FixState, weak: &WeakEntity<App>) -> Div {
    let expanded = state.expanded;
    let header_weak = weak.clone();
    let id = fix.id;
    let mut row = v()
        .when(index > 0, |this| this.border_t(px(1.)).border_color(line()))
        .bg(if expanded {
            surface_2()
        } else {
            alpha(0x00000000)
        });
    row = row.child(
        h().id(("fix-row", index))
            .w_full()
            .items_center()
            .gap(px(18.))
            .px(px(24.))
            .py(px(20.))
            .cursor_pointer()
            .hover(|this| this.bg(alpha(0xffffff06)))
            .on_click(move |_, _, cx| {
                let _ = header_weak.update(cx, |app, cx| app.toggle_fix(id, cx));
            })
            .child(div().w(px(52.)).flex_shrink_0().child(txt(
                fix.number,
                12.,
                FontWeight::NORMAL,
                dim(),
            )))
            .child(
                v().flex_1()
                    .min_w_0()
                    .child(txt(fix.name, 14., FontWeight::NORMAL, text()))
                    .child(txt(fix.subtitle, 11.5, FontWeight::NORMAL, dim()).mt(px(6.))),
            )
            .child(div().w(px(210.)).flex_shrink_0().child(status_cell(state)))
            .child(action_links(fix, weak))
            .child(chevron(expanded)),
    );
    if expanded {
        row = row.child(detail(fix, state));
    }
    row
}

fn status_cell(state: &fixes::FixState) -> Div {
    let (color, dot) = match state.status {
        Status::Applied => (ok(), ok()),
        Status::Healthy => (text(), ok()),
        Status::Attention => (accent(), accent()),
        Status::Unknown => (muted(), warn()),
    };
    let label = if state.busy {
        "执行中…"
    } else {
        state.status.label()
    };
    h().items_center()
        .gap(px(8.))
        .child(div().size(px(5.)).rounded_full().bg(dot))
        .child(txt(label, 11.5, FontWeight::NORMAL, color))
}

fn action_links(fix: &Fix, weak: &WeakEntity<App>) -> Div {
    let mut row = h()
        .w(px(150.))
        .flex_shrink_0()
        .items_center()
        .justify_end()
        .gap(px(14.));
    for (i, action) in [Action::Check, Action::Apply, Action::Revert]
        .into_iter()
        .enumerate()
    {
        let weak = weak.clone();
        let id = fix.id;
        row = row.child(
            div()
                .id(("fix-action", index_key(fix, i)))
                .cursor_pointer()
                .hover(|this| this.text_color(text()).border_color(line_strong()))
                .border_b(px(1.))
                .border_color(alpha(0x00000000))
                .on_click(move |_, _, cx| {
                    let _ = weak.update(cx, |app, cx| app.run_fix(id, action, cx));
                    cx.stop_propagation();
                })
                .child(caps(
                    action.label(),
                    10.5,
                    FontWeight::NORMAL,
                    if action == Action::Apply {
                        accent()
                    } else {
                        dim()
                    },
                    0.14,
                )),
        );
    }
    row
}

fn index_key(fix: &Fix, i: usize) -> usize {
    FIXES.iter().position(|f| f.id == fix.id).unwrap_or(0) * 8 + i
}

fn chevron(expanded: bool) -> Div {
    // A 8px square bordered on two sides reads as a chevron without rotation.
    let (top, right) = if expanded {
        (line_strong(), line_strong())
    } else {
        (dim(), dim())
    };
    div().w(px(26.)).flex_shrink_0().flex().justify_end().child(
        div()
            .size(px(8.))
            .border_t(px(1.))
            .border_color(top)
            .border_r(px(1.))
            .border_color(right),
    )
}

/// Expanded body: evidence + real output | explanation + command.
fn detail(fix: &Fix, state: &fixes::FixState) -> Div {
    h().w_full()
        .items_stretch()
        .border_t(px(1.))
        .border_color(line())
        .child(
            v().flex_1()
                .min_w_0()
                .p(px(26.))
                .border_r(px(1.))
                .border_color(line())
                .child(matrix(fix))
                .child(output_panel(state)),
        )
        .child(
            v().w(relative(0.4))
                .flex_shrink_0()
                .p(px(26.))
                .child(caps("做了什么", 10., FontWeight::NORMAL, dim(), 0.22).mb(px(14.)))
                .child(txt(fix.why, 12., FontWeight::NORMAL, muted()).line_height(px(21.)))
                .child(
                    div()
                        .mt(px(16.))
                        .p(px(12.))
                        .border(px(1.))
                        .border_color(line())
                        .child(txt(fix.command, 11., FontWeight::NORMAL, dim())),
                ),
        )
}

fn matrix(fix: &Fix) -> Div {
    let mut top = h().items_stretch();
    let mut bottom = h().items_stretch();
    for (i, (label, value)) in fix.cells.iter().enumerate() {
        let cell = v()
            .flex_1()
            .min_w_0()
            .p(px(16.))
            .when(i % 2 == 1, |this| {
                this.border_l(px(1.)).border_color(line())
            })
            .child(caps(label, 9.5, FontWeight::NORMAL, dim(), 0.18))
            .child(txt(*value, 11.5, FontWeight::NORMAL, muted()).mt(px(8.)));
        if i < 2 {
            top = top.child(cell);
        } else {
            bottom = bottom.child(cell);
        }
    }
    v().mb(px(18.))
        .border(px(1.))
        .border_color(line())
        .child(top)
        .child(bottom.border_t(px(1.)).border_color(line()))
}

fn output_panel(state: &fixes::FixState) -> Div {
    // Prefer lines that actually carry a diagnosis; skip the box drawing,
    // timestamps and blank lines that the upstream tools print.
    let interesting: Vec<&str> = state
        .output
        .lines()
        .map(str::trim)
        .filter(|line| {
            !line.is_empty()
                && line.chars().count() > 3
                && !line.starts_with('=')
                && !line.starts_with("20")
                && !line.starts_with("─")
        })
        .collect();
    let lines: Vec<String> = if interesting.is_empty() {
        vec!["点「体检」查看外部工具的真实输出".to_string()]
    } else {
        interesting
            .into_iter()
            .take(8)
            .map(|line| {
                if line.chars().count() > 78 {
                    format!("{}…", line.chars().take(78).collect::<String>())
                } else {
                    line.to_string()
                }
            })
            .collect()
    };
    let mut panel = v().gap(px(2.));
    if let Some(outcome) = &state.last {
        let label = format!(
            "{} · {}",
            outcome.action.tag(),
            if outcome.ok { "成功" } else { "失败" }
        );
        panel = panel.child(caps(
            &label,
            10.,
            FontWeight::NORMAL,
            if outcome.ok { ok() } else { accent() },
            0.18,
        ));
    }
    for line in lines {
        panel = panel.child(txt(line, 11., FontWeight::NORMAL, dim()));
    }
    panel
}

fn footer() -> Div {
    h().w_full()
        .items_start()
        .justify_between()
        .gap(px(30.))
        .border_t(px(1.))
        .border_color(line())
        .pt(px(24.))
        .child(
            txt(
                "这些都不是「根治」：每项只是把受影响的程序绕到没坏的通道，或补齐缺失的启动项。所有改动不写系统文件，随时可回退；涉及会话的改动会标注「需重登」。",
                12.,
                FontWeight::NORMAL,
                dim(),
            )
            .line_height(px(22.)),
        )
        .child(
            div().flex_shrink_0()
                .border(px(1.))
                .border_color(line())
                .px(px(10.))
                .py(px(5.))
                .child(caps(
                    "5 项 · 0 项写系统文件",
                    10.,
                    FontWeight::NORMAL,
                    warn(),
                    0.1,
                )),
        )
}
