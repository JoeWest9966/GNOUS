//! Renderer for the `startup-layouts` page-level extension.
//!
//! Three views, mirroring `demo/startup-page/index.html`:
//! `layouts` (page list · screen canvas · inspector), `boot` (timing policy),
//! `journal` (what the executor actually did — empty until it exists).
//!
//! The model, the probes and the JSON round trip live in [`crate::startup`];
//! this file only draws and dispatches.

use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, Div, FontWeight, Hsla, InteractiveElement as _, IntoElement as _,
    ParentElement as _, Stateful, StatefulInteractiveElement as _, Styled, WeakEntity, div, px,
    relative,
};

use crate::startup::{
    self, Display, Page, Preset, Project, Readiness, Split, State as StartupState, Stepper,
    WindowSpec,
};
use crate::theme::{
    accent, accent_hover, alpha, black, caps, dim, h, line, line_faint, line_strong, muted, ok,
    surface_2, text, txt, v, warn, white,
};
use crate::{App, Overview};

const WORKBENCH_HEIGHT: f32 = 560.;

pub fn body(view: &str, overview: &Overview, weak: &WeakEntity<App>) -> Div {
    match view {
        "boot" => boot(overview, weak),
        "journal" => journal(overview, weak),
        "projects" => projects(overview, weak),
        _ => layouts(overview, weak),
    }
}

// ============================================================================
// Shared chrome
// ============================================================================

fn status_line(state: &StartupState) -> Div {
    let (message, color) = if let Some(error) = &state.error {
        (error.clone(), warn())
    } else if let Some(note) = &state.note {
        (note.clone(), muted())
    } else {
        (
            format!(
                "配置已自动保存 · {} · {}",
                state.saved_at,
                startup::State::path().display()
            ),
            dim(),
        )
    };
    h().items_center()
        .gap(px(9.))
        .mb(px(16.))
        .child(div().size(px(5.)).rounded_full().bg(color))
        .child(
            txt(message, 11., FontWeight::NORMAL, color)
                .flex_1()
                .min_w_0()
                .text_ellipsis(),
        )
}

fn strip(state: &StartupState) -> Div {
    let total = state.layout.pages.len();
    let display_value = if state.displays.is_empty() {
        "—".to_string()
    } else {
        state.displays.len().to_string()
    };
    let display_unit = match state.primary() {
        Some(display) => format!("{} · {}×{}", display.name, display.width, display.height),
        None => "未探测到显示器".to_string(),
    };
    h().items_stretch()
        .border_t(px(1.))
        .border_b(px(1.))
        .border_color(line())
        .mb(px(22.))
        .child(stat_cell(
            0,
            "参与开机",
            format!("{}", state.boot_pages()),
            format!("/ {total} 页"),
            text(),
        ))
        .child(stat_cell(
            1,
            "窗口",
            format!("{}", state.boot_windows()),
            String::new(),
            text(),
        ))
        .child(stat_cell(2, "显示器", display_value, display_unit, text()))
        .child(stat_cell(
            3,
            "上次启动",
            "—".to_string(),
            "执行器未接入".to_string(),
            dim(),
        ))
}

fn stat_cell(index: usize, label: &str, value: String, unit: String, color: Hsla) -> Div {
    v().flex_1()
        .min_w_0()
        .py(px(15.))
        .px(if index == 0 { px(0.) } else { px(22.) })
        .when(index > 0, |this| this.border_l(px(1.)).border_color(line()))
        .child(caps(label, 10., FontWeight::NORMAL, dim(), 0.2))
        .child(
            h().items_end()
                .gap(px(6.))
                .mt(px(6.))
                .child(txt(value, 16., FontWeight::LIGHT, color))
                .when(!unit.is_empty(), |this| {
                    this.child(txt(unit, 10.5, FontWeight::NORMAL, dim()))
                }),
        )
}

fn pane_head(label: String, right: Option<Stateful<Div>>) -> Div {
    h().h(px(44.))
        .flex_shrink_0()
        .items_center()
        .justify_between()
        .px(px(16.))
        .border_b(px(1.))
        .border_color(line())
        .child(caps(&label, 10., FontWeight::NORMAL, dim(), 0.2))
        .children(right)
}

fn hint(raw: &str) -> Div {
    div()
        .flex_shrink_0()
        .p(px(14.))
        .border_t(px(1.))
        .border_color(line())
        .child(txt(raw, 10.5, FontWeight::NORMAL, dim()).line_height(px(18.)))
}

fn field(label: &str, value: String, mono: bool) -> Div {
    h().items_center()
        .justify_between()
        .gap(px(12.))
        .py(px(8.))
        .border_b(px(1.))
        .border_color(line_faint())
        .child(txt(label, 10.5, FontWeight::NORMAL, dim()))
        .child(
            txt(
                value,
                if mono { 10.5 } else { 11.5 },
                FontWeight::NORMAL,
                if mono { muted() } else { text() },
            )
            .font_family(if mono {
                "Noto Sans Mono CJK SC"
            } else {
                crate::theme::FONT
            })
            .flex_1()
            .min_w_0()
            .text_ellipsis(),
        )
}

// ============================================================================
// Layouts view
// ============================================================================

fn layouts(overview: &Overview, weak: &WeakEntity<App>) -> Div {
    let state = overview.startup;
    v().child(hero(
        "自研扩展 · startup-layouts · v0.1.0",
        "开机布局",
        "勾选要参与的页面，再决定每页放哪些窗口、怎么分割。执行器接入前，这里只保存配置并预演命令能不能找到。",
        weak,
    ))
        .child(status_line(state))
        .child(strip(state))
        .when_some(state.preview.as_ref(), |this, preview| {
            this.child(preview_panel(preview))
        })
        .child(workbench(overview, weak))
        .child(footer())
}

fn hero(eyebrow: &str, title: &str, summary: &str, weak: &WeakEntity<App>) -> Div {
    let preview_weak = weak.clone();
    let save_weak = weak.clone();
    v().mb(px(26.))
        .child(caps(eyebrow, 11., FontWeight::NORMAL, dim(), 0.26).mb(px(14.)))
        .child(
            h().w_full()
                .items_end()
                .justify_between()
                .gap(px(24.))
                .child(
                    v().flex_1()
                        .min_w_0()
                        .child(
                            txt(title, 44., FontWeight::EXTRA_LIGHT, text()).line_height(px(46.)),
                        )
                        .child(txt(summary, 13., FontWeight::NORMAL, muted()).mt(px(12.))),
                )
                .child(
                    h().items_center()
                        .gap(px(10.))
                        .child(hero_button("预演一遍", false, move |cx| {
                            let _ = preview_weak.update(cx, |app, cx| app.startup_preview(cx));
                        }))
                        .child(hero_button("保存并应用", true, move |cx| {
                            let _ = save_weak.update(cx, |app, cx| app.startup_save(cx));
                        })),
                ),
        )
}

fn hero_button(
    label: &'static str,
    solid: bool,
    handler: impl Fn(&mut gpui_kit::App) + 'static,
) -> Stateful<Div> {
    let (bg, border, fg) = if solid {
        (accent(), accent(), white())
    } else {
        (alpha(0x00000000), line_strong(), text())
    };
    div()
        .id(label)
        .flex()
        .items_center()
        .justify_center()
        .px(px(22.))
        .py(px(11.))
        .border(px(1.))
        .border_color(border)
        .bg(bg)
        .text_color(fg)
        .cursor_pointer()
        .hover(move |this| {
            if solid {
                this.bg(accent_hover()).border_color(accent_hover())
            } else {
                this.bg(alpha(0xffffff14)).border_color(line_strong())
            }
        })
        .child(caps(label, 11., FontWeight::NORMAL, fg, 0.16))
        .on_click(move |_, _, cx| handler(cx))
}

fn preview_panel(preview: &startup::Preview) -> Div {
    let mut panel = v().border(px(1.)).border_color(line()).mb(px(22.)).child(
        h().items_center()
            .justify_between()
            .px(px(16.))
            .py(px(12.))
            .border_b(px(1.))
            .border_color(line())
            .child(caps(
                "预演 · 只检查命令可执行性",
                10.,
                FontWeight::NORMAL,
                dim(),
                0.18,
            ))
            .child(caps(
                &format!(
                    "{} · {} 可执行 / {} 缺失 / {} 跳过",
                    preview.at,
                    preview.ok_count(),
                    preview.miss_count(),
                    preview.skip_count()
                ),
                10.,
                FontWeight::NORMAL,
                if preview.miss_count() == 0 {
                    ok()
                } else {
                    warn()
                },
                0.1,
            )),
    );
    for line in &preview.lines {
        panel = panel.child(
            h().items_center()
                .justify_between()
                .gap(px(16.))
                .px(px(16.))
                .py(px(8.))
                .border_b(px(1.))
                .border_color(line_faint())
                .child(txt(line.head.clone(), 11.5, FontWeight::NORMAL, text()))
                .child(txt(
                    line.body.clone(),
                    10.5,
                    FontWeight::NORMAL,
                    if line.ok { dim() } else { warn() },
                )),
        );
    }
    panel
}

fn workbench(overview: &Overview, weak: &WeakEntity<App>) -> Div {
    h().w_full()
        .h(px(WORKBENCH_HEIGHT))
        .items_stretch()
        .border(px(1.))
        .border_color(line())
        .child(page_pane(overview, weak))
        .child(canvas_pane(overview, weak))
        .child(inspector_pane(overview, weak))
}

fn page_pane(overview: &Overview, weak: &WeakEntity<App>) -> Div {
    let state = overview.startup;
    let new_weak = weak.clone();
    let mut rows = v();
    for (index, page) in state.layout.pages.iter().enumerate() {
        rows = rows.child(page_row(index, page, page.id == state.selected, weak));
    }
    v().w(px(250.))
        .flex_shrink_0()
        .border_r(px(1.))
        .border_color(line())
        .child(pane_head(
            format!("页面 · {}", state.layout.pages.len()),
            Some(
                div()
                    .id("startup-new-page")
                    .cursor_pointer()
                    .hover(|this| this.bg(alpha(0xffffff14)))
                    .on_click(move |_, _, cx| {
                        let _ = new_weak.update(cx, |app, cx| app.startup_new_page(cx));
                    })
                    .child(caps("＋ 新建", 10., FontWeight::NORMAL, muted(), 0.16)),
            ),
        ))
        .child(
            v().id("startup-pages")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .child(rows),
        )
        .child(hint(
            "勾选框 = 开机是否铺开这个页面。未勾选的页面不会创建，也不会切过去。",
        ))
}

fn page_row(index: usize, page: &Page, selected: bool, weak: &WeakEntity<App>) -> Stateful<Div> {
    let select_weak = weak.clone();
    let check_weak = weak.clone();
    let id = page.id.clone();
    let check_id = page.id.clone();
    let boot = page.boot;
    let meta = format!(
        "{} 窗口 · {}",
        page.windows.len(),
        if page.windows.is_empty() {
            "空白".to_string()
        } else {
            page.split.label().to_string()
        }
    );
    h().id(("startup-page", index))
        .relative()
        .items_center()
        .gap(px(12.))
        .px(px(16.))
        .py(px(13.))
        .cursor_pointer()
        .border_b(px(1.))
        .border_color(line_faint())
        .when(selected, |this| {
            this.bg(surface_2()).child(
                div()
                    .absolute()
                    .left(px(0.))
                    .top(px(0.))
                    .bottom(px(0.))
                    .w(px(2.))
                    .bg(accent()),
            )
        })
        .hover(|this| this.bg(surface_2()))
        .on_click(move |_, _, cx| {
            let id = id.clone();
            let _ = select_weak.update(cx, |app, cx| app.startup_select_page(&id, cx));
        })
        .child(
            div()
                .id(("startup-chk", index))
                .size(px(14.))
                .flex_shrink_0()
                .flex()
                .items_center()
                .justify_center()
                .border(px(1.))
                .border_color(if boot { text() } else { line_strong() })
                .bg(if boot { text() } else { alpha(0x00000000) })
                .cursor_pointer()
                .on_click(move |_, _, cx| {
                    cx.stop_propagation();
                    let id = check_id.clone();
                    let _ = check_weak.update(cx, |app, cx| app.startup_toggle_boot(&id, cx));
                })
                .child(
                    div()
                        .text_size(px(9.))
                        .line_height(px(11.))
                        .font_weight(FontWeight::BOLD)
                        .text_color(black())
                        .when(boot, |this| this.child("✓")),
                ),
        )
        .child(
            v().flex_1()
                .min_w_0()
                .child(
                    h().items_center()
                        .gap(px(7.))
                        .child(caps(
                            &format!("{:02}", index + 1),
                            9.5,
                            FontWeight::NORMAL,
                            dim(),
                            0.14,
                        ))
                        .child(txt(
                            page.name.clone(),
                            13.,
                            FontWeight::NORMAL,
                            if boot { text() } else { dim() },
                        )),
                )
                .child(
                    txt(meta, 10.5, FontWeight::NORMAL, dim())
                        .mt(px(3.))
                        .when(!boot, |this| this.text_color(alpha(0xffffff36))),
                ),
        )
        .child(mini(page))
}

/// 40×26 thumbnail: one bar per window, first one accented.
fn mini(page: &Page) -> Div {
    let count = page.windows.len();
    let mut bars = match page.split {
        Split::Rows(_) => v(),
        _ => h(),
    }
    .w(px(32.))
    .h(px(16.))
    .gap(px(1.));
    for index in 0..count.max(1) {
        bars = bars.child(div().flex_1().min_w_0().bg(if index == 0 {
            alpha(0xffffff24)
        } else {
            alpha(0xffffff14)
        }));
    }
    div()
        .w(px(40.))
        .h(px(26.))
        .flex_shrink_0()
        .flex()
        .items_center()
        .justify_center()
        .border(px(1.))
        .border_color(if page.boot { line_strong() } else { line() })
        .child(bars)
}

fn canvas_pane(overview: &Overview, weak: &WeakEntity<App>) -> Div {
    let state = overview.startup;
    let index = state.selected_index();
    let title = match state.page() {
        Some(page) => format!("{:02} · {}", index + 1, page.name),
        None => "未选中页面".to_string(),
    };
    v().flex_1()
        .min_w_0()
        .child(pane_head(
            title,
            Some(div().id("startup-screen-badge").child(caps(
                "屏幕预览",
                9.5,
                FontWeight::NORMAL,
                dim(),
                0.14,
            ))),
        ))
        .child(
            v().flex_1()
                .min_h_0()
                .p(px(16.))
                .child(preset_bar(state, weak))
                .child(screen(overview, weak))
                .child(canvas_foot(state)),
        )
}

fn preset_bar(state: &StartupState, weak: &WeakEntity<App>) -> Div {
    let windows = state.page().map(|page| page.windows.len()).unwrap_or(0);
    let current = state
        .page()
        .map(|page| page.split.preset(page.windows.len()));
    let mut group = h().border(px(1.)).border_color(line());
    for preset in Preset::ALL {
        let enabled = preset.enabled(windows);
        let active = current == Some(preset);
        let weak = weak.clone();
        group = group.child(
            div()
                .id(("startup-preset", preset as usize))
                .px(px(13.))
                .py(px(7.))
                .border_l(px(1.))
                .border_color(if preset == Preset::Solo {
                    alpha(0x00000000)
                } else {
                    line()
                })
                .when(active, |this| this.bg(text()))
                .when(enabled, |this| {
                    this.cursor_pointer()
                        .hover(|this| this.bg(if active { text() } else { alpha(0xffffff0a) }))
                        .on_click(move |_, _, cx| {
                            let _ = weak.update(cx, |app, cx| app.startup_preset(preset, cx));
                        })
                })
                .when(!enabled, |this| this.opacity(0.3))
                .child(caps(
                    preset.label(),
                    10.5,
                    FontWeight::NORMAL,
                    if active { black() } else { muted() },
                    0.1,
                )),
        );
    }
    h().items_center()
        .gap(px(14.))
        .mb(px(14.))
        .child(group)
        .child(
            txt(
                match (state.primary(), state.page()) {
                    (Some(display), _) => format!(
                        "{} · {}×{} · {}",
                        display.name, display.width, display.height, display.source
                    ),
                    (None, _) => "显示器未知 · 画布按比例显示".to_string(),
                },
                10.,
                FontWeight::NORMAL,
                dim(),
            )
            .flex_1()
            .min_w_0(),
        )
}

fn screen(overview: &Overview, weak: &WeakEntity<App>) -> Div {
    let state = overview.startup;
    let Some(page) = state.page() else {
        return v()
            .flex_1()
            .min_h_0()
            .items_center()
            .justify_center()
            .border(px(1.))
            .border_color(line_strong())
            .child(caps("未选中页面", 11., FontWeight::NORMAL, dim(), 0.16));
    };
    let slots = startup::slots(page);
    if slots.is_empty() {
        return v()
            .flex_1()
            .min_h_0()
            .items_center()
            .justify_center()
            .border(px(1.))
            .border_color(line_strong())
            .bg(alpha(0xffffff05))
            .child(
                v().items_center()
                    .child(caps("这一页还是空的", 11., FontWeight::NORMAL, dim(), 0.16))
                    .child(
                        txt(
                            "窗口编辑（添加 / 换应用）下一步接入，可先直接改 JSON",
                            10.5,
                            FontWeight::NORMAL,
                            dim(),
                        )
                        .mt(px(8.)),
                    ),
            );
    }
    let display = state.primary();
    let selected = state.window;
    let cards: Vec<Stateful<Div>> = page
        .windows
        .iter()
        .enumerate()
        .map(|(index, window)| {
            win_card(
                index,
                window,
                slots[index],
                display,
                index == selected,
                weak,
            )
        })
        .collect();

    let body = match page.split {
        Split::Rows(_) => {
            v().size_full()
                .gap(px(1.))
                .children(cards.into_iter().zip(slots.iter()).map(|(card, slot)| {
                    div()
                        .flex_basis(relative(slot.h))
                        .min_h_0()
                        .flex()
                        .child(card)
                }))
        }
        Split::Grid => {
            let mut rows = v().size_full().gap(px(1.));
            let mut cards = cards.into_iter();
            for _ in 0..2 {
                let mut row = h().flex_1().min_h_0().gap(px(1.));
                for _ in 0..2 {
                    if let Some(card) = cards.next() {
                        row = row.child(div().flex_1().min_w_0().flex().child(card));
                    }
                }
                rows = rows.child(row);
            }
            rows
        }
        Split::Columns(_) => {
            h().size_full()
                .gap(px(1.))
                .children(cards.into_iter().zip(slots.iter()).map(|(card, slot)| {
                    div()
                        .flex_basis(relative(slot.w))
                        .min_w_0()
                        .flex()
                        .child(card)
                }))
        }
    };

    div()
        .flex_1()
        .min_h_0()
        .flex()
        .bg(line_strong())
        .p(px(1.))
        .child(body)
}

fn win_card(
    index: usize,
    window: &WindowSpec,
    slot: startup::Slot,
    display: Option<&Display>,
    selected: bool,
    weak: &WeakEntity<App>,
) -> Stateful<Div> {
    let weak = weak.clone();
    let geo = match display {
        Some(display) => slot.pixels(display),
        None => slot.percent(),
    };
    v().id(("startup-win", index))
        .flex_1()
        .min_w_0()
        .min_h_0()
        .gap(px(6.))
        .p(px(10.))
        .bg(if selected { surface_2() } else { black() })
        .when(selected, |this| this.border(px(1.)).border_color(accent()))
        .when(!selected, |this| {
            this.border(px(1.)).border_color(alpha(0x00000000))
        })
        .cursor_pointer()
        .hover(|this| this.bg(surface_2()))
        .on_click(move |_, _, cx| {
            let _ = weak.update(cx, |app, cx| app.startup_select_window(index, cx));
        })
        .child(
            h().items_center()
                .gap(px(8.))
                .min_w_0()
                .child(
                    div()
                        .size(px(20.))
                        .flex_shrink_0()
                        .flex()
                        .items_center()
                        .justify_center()
                        .border(px(1.))
                        .border_color(if selected { accent() } else { line_strong() })
                        .child(txt(
                            window.tag.clone(),
                            8.5,
                            FontWeight::NORMAL,
                            if selected { accent() } else { muted() },
                        )),
                )
                .child(
                    txt(window.app.clone(), 12., FontWeight::NORMAL, text())
                        .flex_1()
                        .min_w_0()
                        .text_ellipsis(),
                )
                .child(caps(
                    window.role.label(),
                    8.5,
                    FontWeight::NORMAL,
                    dim(),
                    0.12,
                )),
        )
        .child(
            txt(window.command.clone(), 9.5, FontWeight::NORMAL, dim())
                .font_family("Noto Sans Mono CJK SC")
                .text_ellipsis(),
        )
        .child(div().flex_1())
        .child(txt(geo, 9.5, FontWeight::NORMAL, dim()))
}

fn canvas_foot(state: &StartupState) -> Div {
    let (count, split) = match state.page() {
        Some(page) => (page.windows.len(), page.split.label()),
        None => (0, "—"),
    };
    h().items_center()
        .justify_between()
        .mt(px(12.))
        .child(txt(
            "点击窗口选中 · 排列只存比例，换分辨率后按比例重算",
            10.5,
            FontWeight::NORMAL,
            dim(),
        ))
        .child(caps(
            &format!("{count} 窗口 · {split}"),
            10.,
            FontWeight::NORMAL,
            dim(),
            0.08,
        ))
}

fn inspector_pane(overview: &Overview, weak: &WeakEntity<App>) -> Div {
    v().w(px(304.))
        .flex_shrink_0()
        .border_l(px(1.))
        .border_color(line())
        .child(pane_head("检查器".to_string(), None))
        .child(
            v().id("startup-inspector")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .children(inspector_groups(overview, weak)),
        )
}

fn inspector_groups(overview: &Overview, weak: &WeakEntity<App>) -> Vec<AnyElement> {
    let state = overview.startup;
    let Some(page) = state.page() else {
        return vec![
            v().p(px(16.))
                .child(txt("未选中页面", 11.5, FontWeight::NORMAL, dim()))
                .into_any_element(),
        ];
    };
    let mut groups = Vec::new();

    if let Some(window) = page.windows.get(state.window) {
        let index = state.window;
        let slot = startup::slots(page).get(index).copied();
        let geo = match (slot, state.primary()) {
            (Some(slot), Some(display)) => slot.pixels(display),
            (Some(slot), None) => slot.percent(),
            _ => "—".to_string(),
        };
        let remove_weak = weak.clone();
        groups.push(
            v().p(px(16.))
                .child(
                    h().items_center()
                        .justify_between()
                        .mb(px(12.))
                        .child(caps(
                            &format!("窗口 {:02}", index + 1),
                            10.,
                            FontWeight::NORMAL,
                            dim(),
                            0.2,
                        ))
                        .child(caps(
                            &format!("/ {}", page.windows.len()),
                            10.,
                            FontWeight::NORMAL,
                            alpha(0xffffff24),
                            0.1,
                        )),
                )
                .child(field("应用", window.app.clone(), false))
                .child(field("来源", window.source.clone(), false))
                .child(field("启动命令", window.command.clone(), true))
                .child(field("窗口状态", window.role.label().to_string(), false))
                .child(field("启动延迟", format!("{} ms", window.delay_ms), false))
                .child(field("几何", geo, true))
                .child(
                    div()
                        .id("startup-remove-window")
                        .mt(px(14.))
                        .py(px(9.))
                        .flex()
                        .justify_center()
                        .border(px(1.))
                        .border_color(alpha(0xe3193759))
                        .cursor_pointer()
                        .hover(|this| this.bg(alpha(0xe3193724)))
                        .on_click(move |_, _, cx| {
                            let _ = remove_weak.update(cx, |app, cx| app.startup_remove_window(cx));
                        })
                        .child(caps("移除此窗口", 10., FontWeight::NORMAL, accent(), 0.16)),
                )
                .into_any_element(),
        );
    }

    let ratios = match &page.split {
        Split::Columns(ratios) | Split::Rows(ratios) => ratios
            .iter()
            .map(|ratio| format!("{}", (ratio * 100.).round() as i32))
            .collect::<Vec<_>>()
            .join(" : "),
        Split::Grid => "50 : 50 × 50 : 50".to_string(),
    };
    groups.push(
        v().p(px(16.))
            .border_t(px(1.))
            .border_color(line())
            .child(
                h().items_center()
                    .justify_between()
                    .mb(px(12.))
                    .child(caps("页面", 10., FontWeight::NORMAL, dim(), 0.2))
                    .child(caps(
                        &format!("{:02}", state.selected_index() + 1),
                        10.,
                        FontWeight::NORMAL,
                        alpha(0xffffff24),
                        0.1,
                    )),
            )
            .child(field("名称", page.name.clone(), false))
            .child(field(
                "参与开机",
                if page.boot { "是" } else { "否" }.to_string(),
                false,
            ))
            .child(field(
                "排列",
                format!("{} · {}", page.split.label(), ratios),
                false,
            ))
            .into_any_element(),
    );

    groups
}

fn footer() -> Div {
    h().w_full()
        .items_start()
        .justify_between()
        .gap(px(30.))
        .border_t(px(1.))
        .border_color(line())
        .mt(px(34.))
        .pt(px(22.))
        .child(
            txt(
                "这一版只做配置与预演：勾选、排列、命令解析结果都会真实落盘并重新探测；真正开窗的执行器尚未实现，所以不显示「上次启动」的假时间线。",
                12.,
                FontWeight::NORMAL,
                dim(),
            )
            .line_height(px(22.)),
        )
        .child(
            div()
                .flex_shrink_0()
                .border(px(1.))
                .border_color(line())
                .px(px(10.))
                .py(px(5.))
                .child(caps("JSON · 自动保存", 10., FontWeight::NORMAL, muted(), 0.1)),
        )
}

// ============================================================================
// Boot view
// ============================================================================

fn boot(overview: &Overview, weak: &WeakEntity<App>) -> Div {
    let state = overview.startup;
    let checks_weak = weak.clone();
    v().child(hero(
        "Startup · 执行时机",
        "启动策略",
        "铺开得太早会和会话初始化抢资源，太晚则看得见窗口一个个跳出来。这里定时机与失败处理。",
        weak,
    ))
    .child(status_line(state))
    .child(strip(state))
    .child(
        h().items_stretch()
            .border(px(1.))
            .border_color(line())
            .child(
                v().flex_1()
                    .min_w_0()
                    .p(px(24.))
                    .border_r(px(1.))
                    .border_color(line())
                    .child(caps("时机", 11., FontWeight::MEDIUM, muted(), 0.22))
                    .child(
                        txt(
                            "先等 shell 与会话总线稳定，再开始逐页铺开；单页失败不阻塞下一页。",
                            11.,
                            FontWeight::NORMAL,
                            dim(),
                        )
                        .mt(px(8.))
                        .mb(px(18.)),
                    )
                    .children(boot_rows(state, weak)),
            )
            .child(
                v().flex_1()
                    .min_w_0()
                    .p(px(24.))
                    .child(caps("显示器", 11., FontWeight::MEDIUM, muted(), 0.22))
                    .child(
                        txt(
                            "布局按逻辑显示器存放比例；多屏时优先在指定屏幕上铺。",
                            11.,
                            FontWeight::NORMAL,
                            dim(),
                        )
                        .mt(px(8.))
                        .mb(px(18.)),
                    )
                    .children(display_rows(state, weak))
                    .child(
                        div()
                            .id("startup-recheck-displays")
                            .mt(px(18.))
                            .px(px(14.))
                            .py(px(9.))
                            .border(px(1.))
                            .border_color(line_strong())
                            .cursor_pointer()
                            .hover(|this| this.bg(alpha(0xffffff0a)))
                            .on_click(move |_, _, cx| {
                                let _ = checks_weak
                                    .update(cx, |app, cx| app.startup_recheck_displays(cx));
                            })
                            .child(caps(
                                "重新探测显示器",
                                10.,
                                FontWeight::NORMAL,
                                muted(),
                                0.14,
                            )),
                    ),
            ),
    )
    .child(footer())
}

fn boot_rows(state: &StartupState, weak: &WeakEntity<App>) -> Vec<AnyElement> {
    let mut rows: Vec<AnyElement> = Stepper::ALL
        .iter()
        .map(|stepper| stepper_row(state, *stepper, weak).into_any_element())
        .collect();
    for option in startup::BootOption::ALL {
        rows.push(option_row(state, option, weak).into_any_element());
    }
    rows
}

fn option_row(
    state: &StartupState,
    option: startup::BootOption,
    weak: &WeakEntity<App>,
) -> Stateful<Div> {
    let on = match option {
        startup::BootOption::WaitReady => state.layout.boot.wait_ready,
        startup::BootOption::EveryLogin => state.layout.boot.every_login,
        startup::BootOption::Degrade => state.layout.boot.degrade,
        startup::BootOption::PadWorkspaces => state.layout.boot.pad_workspaces,
    };
    let weak = weak.clone();
    h().items_center()
        .id(("startup-option", option as usize))
        .justify_between()
        .gap(px(16.))
        .py(px(13.))
        .cursor_pointer()
        .border_b(px(1.))
        .border_color(line_faint())
        .hover(|this| this.bg(alpha(0xffffff06)))
        .on_click(move |_, _, cx| {
            let _ = weak.update(cx, |app, cx| app.startup_toggle_option(option, cx));
        })
        .child(
            v().child(txt(option.label(), 12.5, FontWeight::NORMAL, text()))
                .child(txt(option.description(), 10.5, FontWeight::NORMAL, dim()).mt(px(4.))),
        )
        .child(toggle_pill(on))
}

fn display_rows(state: &StartupState, weak: &WeakEntity<App>) -> Vec<AnyElement> {
    let mut rows: Vec<AnyElement> = Vec::new();
    if state.displays.is_empty() {
        rows.push(
            v().py(px(13.))
                .child(txt(
                    "未探测到已连接显示器",
                    12.,
                    FontWeight::NORMAL,
                    muted(),
                ))
                .child(
                    txt(
                        "/sys/class/drm 里没有 connected 的输出；画布改用百分比",
                        10.5,
                        FontWeight::NORMAL,
                        dim(),
                    )
                    .mt(px(4.)),
                )
                .into_any_element(),
        );
        return rows;
    }
    for (index, display) in state.displays.iter().enumerate() {
        let primary = state.layout.boot.primary_display == display.name;
        let weak = weak.clone();
        let name = display.name.clone();
        rows.push(
            h().id(("startup-display", index))
                .items_center()
                .justify_between()
                .gap(px(16.))
                .py(px(13.))
                .cursor_pointer()
                .border_b(px(1.))
                .border_color(line_faint())
                .hover(|this| this.bg(alpha(0xffffff06)))
                .on_click(move |_, _, cx| {
                    let name = name.clone();
                    let _ = weak.update(cx, |app, cx| app.startup_pick_display(&name, cx));
                })
                .child(
                    v().child(txt(
                        format!("{} · {}×{}", display.name, display.width, display.height),
                        12.5,
                        FontWeight::NORMAL,
                        text(),
                    ))
                    .child(txt(display.source, 10.5, FontWeight::NORMAL, dim()).mt(px(4.))),
                )
                .child(if primary {
                    caps("主屏", 9.5, FontWeight::NORMAL, ok(), 0.14)
                } else {
                    caps("设为铺开屏", 9.5, FontWeight::NORMAL, dim(), 0.12)
                })
                .into_any_element(),
        );
    }
    rows
}

fn stepper_row(state: &StartupState, stepper: Stepper, weak: &WeakEntity<App>) -> Div {
    let down_weak = weak.clone();
    let up_weak = weak.clone();
    h().items_center()
        .justify_between()
        .gap(px(16.))
        .py(px(13.))
        .border_b(px(1.))
        .border_color(line_faint())
        .child(
            v().child(txt(stepper.label(), 12.5, FontWeight::NORMAL, text()))
                .child(txt(stepper.description(), 10.5, FontWeight::NORMAL, dim()).mt(px(4.))),
        )
        .child(
            h().items_center()
                .gap(px(8.))
                .child(h().w(px(56.)).justify_end().child(txt(
                    stepper.value(&state.layout.boot),
                    11.5,
                    FontWeight::NORMAL,
                    muted(),
                )))
                .child(step_button(
                    "−",
                    format!("startup-step-down-{}", stepper as usize),
                    move |cx| {
                        let _ =
                            down_weak.update(cx, |app, cx| app.startup_step(stepper, false, cx));
                    },
                ))
                .child(step_button(
                    "+",
                    format!("startup-step-up-{}", stepper as usize),
                    move |cx| {
                        let _ = up_weak.update(cx, |app, cx| app.startup_step(stepper, true, cx));
                    },
                )),
        )
}

fn step_button(
    label: &'static str,
    id: String,
    handler: impl Fn(&mut gpui_kit::App) + 'static,
) -> Stateful<Div> {
    div()
        .id(id)
        .size(px(24.))
        .flex()
        .items_center()
        .justify_center()
        .border(px(1.))
        .border_color(line_strong())
        .cursor_pointer()
        .hover(|this| this.bg(alpha(0xffffff14)))
        .child(caps(label, 11., FontWeight::NORMAL, muted(), 0.04))
        .on_click(move |_, _, cx| handler(cx))
}

fn toggle_pill(on: bool) -> Div {
    div()
        .relative()
        .w(px(46.))
        .h(px(22.))
        .flex_shrink_0()
        .rounded_full()
        .border(px(1.))
        .border_color(if on { alpha(0xffffff8c) } else { line_strong() })
        .bg(if on {
            alpha(0xffffff14)
        } else {
            alpha(0xffffff05)
        })
        .child(
            div()
                .absolute()
                .top(px(3.))
                .left(px(if on { 27. } else { 3. }))
                .w(px(14.))
                .h(px(14.))
                .rounded_full()
                .bg(if on { text() } else { muted() }),
        )
}

// ============================================================================
// Projects view
// ============================================================================

fn projects(overview: &Overview, weak: &WeakEntity<App>) -> Div {
    let state = overview.startup;
    v().child(projects_hero(weak))
        .child(status_line(state))
        .child(projects_strip(state))
        .child(
            h().items_stretch()
                .border(px(1.))
                .border_color(line())
                .h(px(WORKBENCH_HEIGHT))
                .child(project_pane(overview, weak))
                .child(script_pane(overview, weak))
                .child(project_inspector_pane(overview, weak)),
        )
        .child(projects_footer())
}

fn projects_hero(weak: &WeakEntity<App>) -> Div {
    let rescan_weak = weak.clone();
    let open_weak = weak.clone();
    v().mb(px(26.))
        .child(
            caps("Startup · 自研项目", 11., FontWeight::NORMAL, dim(), 0.26).mb(px(14.)),
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
                            txt("项目", 44., FontWeight::EXTRA_LIGHT, text())
                                .line_height(px(46.)),
                        )
                        .child(
                            txt(
                                "你自己写的启动项目脚本：每个项目一个目录、一份清单、一支脚本。这里只登记、读取、编排，脚本执行器尚未接入。",
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
                        .child(hero_button("重新扫描", false, move |cx| {
                            let _ = rescan_weak.update(cx, |app, cx| app.startup_rescan_projects(cx));
                        }))
                        .child(hero_button("打开项目目录", true, move |cx| {
                            let _ = open_weak.update(cx, |app, cx| app.startup_open_projects(cx));
                        })),
                ),
        )
        .child(
            h().items_center()
                .gap(px(8.))
                .mt(px(14.))
                .child(caps("根目录", 9.5, FontWeight::NORMAL, dim(), 0.18))
                .child(
                    txt(
                        startup::projects_root().display().to_string(),
                        11.,
                        FontWeight::NORMAL,
                        muted(),
                    )
                    .font_family("Noto Sans Mono CJK SC")
                    .flex_1()
                    .min_w_0()
                    .text_ellipsis(),
                ),
        )
}

fn projects_strip(state: &StartupState) -> Div {
    let total = state.projects.len();
    let broken = state
        .projects
        .iter()
        .filter(|project| !project.readiness.is_ready())
        .count();
    let ready = state.ready_projects();
    h().items_stretch()
        .border_t(px(1.))
        .border_b(px(1.))
        .border_color(line())
        .mb(px(22.))
        .child(stat_cell(
            0,
            "项目",
            format!("{total}"),
            if total == 0 {
                "个".to_string()
            } else {
                "个目录".to_string()
            },
            text(),
        ))
        .child(stat_cell(
            1,
            "随开机启动",
            format!("{}", state.boot_projects()),
            format!("/ {total} 个"),
            text(),
        ))
        .child(stat_cell(
            2,
            "可直接运行",
            format!("{ready}"),
            if broken == 0 {
                format!("/ {total} 个")
            } else {
                format!("· {broken} 个待修")
            },
            if total == 0 {
                dim()
            } else if broken == 0 {
                ok()
            } else {
                warn()
            },
        ))
        .child(stat_cell(
            3,
            "扫描",
            state.projects_at.clone(),
            "重新扫描前不会自动更新".to_string(),
            dim(),
        ))
}

fn project_pane(overview: &Overview, weak: &WeakEntity<App>) -> Div {
    let state = overview.startup;
    let mut rows = v();
    for (index, project) in state.projects.iter().enumerate() {
        rows = rows.child(project_row(
            index,
            project,
            project.id == state.project,
            weak,
        ));
    }
    v().w(px(250.))
        .flex_shrink_0()
        .border_r(px(1.))
        .border_color(line())
        .child(pane_head(format!("项目 · {}", state.projects.len()), None))
        .child(if state.projects.is_empty() {
            v().flex_1()
                .min_h_0()
                .p(px(16.))
                .child(caps("还没有项目", 10.5, FontWeight::NORMAL, muted(), 0.2))
                .child(
                    txt(
                        "在根目录下建一个文件夹，放入 project.toml 和脚本，再点「重新扫描」。右边有模板。",
                        11.,
                        FontWeight::NORMAL,
                        dim(),
                    )
                    .line_height(px(18.))
                    .mt(px(10.)),
                )
                .into_any_element()
        } else {
            v().id("startup-projects")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .child(rows)
                .into_any_element()
        })
        .child(hint(
            "勾选框 = 随开机启动。这里改的是项目自己的 project.toml，点一次写入一次。",
        ))
}

fn project_row(
    index: usize,
    project: &Project,
    selected: bool,
    weak: &WeakEntity<App>,
) -> Stateful<Div> {
    let select_weak = weak.clone();
    let check_weak = weak.clone();
    let id = project.id.clone();
    let check_id = project.id.clone();
    let boot = project.boot;
    let dot = readiness_color(project.readiness);
    let meta = if let Some(problem) = &project.problem {
        problem.clone()
    } else if project.summary.is_empty() {
        format!("{} · {}", project.script, project.readiness.label())
    } else {
        project.summary.clone()
    };
    h().id(("startup-project", index))
        .relative()
        .items_center()
        .gap(px(12.))
        .px(px(16.))
        .py(px(13.))
        .cursor_pointer()
        .border_b(px(1.))
        .border_color(line_faint())
        .when(selected, |this| {
            this.bg(surface_2()).child(
                div()
                    .absolute()
                    .left(px(0.))
                    .top(px(0.))
                    .bottom(px(0.))
                    .w(px(2.))
                    .bg(accent()),
            )
        })
        .hover(|this| this.bg(surface_2()))
        .on_click(move |_, _, cx| {
            let id = id.clone();
            let _ = select_weak.update(cx, |app, cx| app.startup_select_project(&id, cx));
        })
        .child(
            div()
                .id(("startup-project-chk", index))
                .size(px(14.))
                .flex_shrink_0()
                .flex()
                .items_center()
                .justify_center()
                .border(px(1.))
                .border_color(if boot { text() } else { line_strong() })
                .bg(if boot { text() } else { alpha(0x00000000) })
                .cursor_pointer()
                .on_click(move |_, _, cx| {
                    cx.stop_propagation();
                    let id = check_id.clone();
                    let _ =
                        check_weak.update(cx, |app, cx| app.startup_toggle_project_boot(&id, cx));
                })
                .child(
                    div()
                        .text_size(px(9.))
                        .line_height(px(11.))
                        .font_weight(FontWeight::BOLD)
                        .text_color(black())
                        .when(boot, |this| this.child("✓")),
                ),
        )
        .child(
            v().flex_1()
                .min_w_0()
                .child(
                    h().items_center()
                        .gap(px(7.))
                        .child(caps(
                            &format!("{:02}", index + 1),
                            9.5,
                            FontWeight::NORMAL,
                            dim(),
                            0.14,
                        ))
                        .child(
                            txt(project.name.clone(), 13., FontWeight::NORMAL, text())
                                .flex_1()
                                .min_w_0()
                                .text_ellipsis(),
                        ),
                )
                .child(
                    txt(meta, 10.5, FontWeight::NORMAL, dim())
                        .mt(px(3.))
                        .when(!boot, |this| this.text_color(alpha(0xffffff36)))
                        .text_ellipsis(),
                ),
        )
        .child(div().size(px(6.)).flex_shrink_0().rounded_full().bg(dot))
}

fn readiness_color(readiness: Readiness) -> Hsla {
    match readiness {
        Readiness::Ready => ok(),
        Readiness::NotExecutable => warn(),
        Readiness::Missing => warn(),
        Readiness::Broken => accent(),
    }
}

fn script_pane(overview: &Overview, weak: &WeakEntity<App>) -> Div {
    let state = overview.startup;
    let pane = v().flex_1().min_w_0().border_r(px(1.)).border_color(line());
    match state.selected_project() {
        None => pane
            .child(pane_head("脚本".to_string(), None))
            .child(empty_script(state)),
        Some(project) => {
            let copy_weak = weak.clone();
            let command = project.command();
            let head = if project.lines > 0 {
                format!("{}/{} · {} 行", project.id, project.script, project.lines)
            } else {
                format!("{}/{}", project.id, project.script)
            };
            pane.child(pane_head(
                head,
                Some(
                    div()
                        .id("startup-project-copy")
                        .cursor_pointer()
                        .hover(|this| this.bg(alpha(0xffffff14)))
                        .px(px(8.))
                        .py(px(3.))
                        .on_click(move |_, _, cx| {
                            let command = command.clone();
                            let _ = copy_weak
                                .update(cx, |app, cx| app.copy_text(command, "启动命令", cx));
                        })
                        .child(caps("复制命令", 9.5, FontWeight::NORMAL, muted(), 0.16)),
                ),
            ))
            .child(script_body(project))
            .child(hint(
                "只读预览，最多 400 行。执行器接入前，这里不会运行脚本。",
            ))
        }
    }
}

fn empty_script(state: &StartupState) -> Div {
    let mut template = v();
    for (index, line) in startup::PROJECT_TEMPLATE.lines().enumerate() {
        template = template.child(
            txt(
                line.to_string(),
                if line.starts_with('#') { 10.5 } else { 11. },
                FontWeight::NORMAL,
                if line.starts_with('#') {
                    dim()
                } else {
                    muted()
                },
            )
            .font_family("Noto Sans Mono CJK SC")
            .line_height(px(18.))
            .when(index == 0, |this| this.mb(px(6.))),
        );
    }
    v().flex_1()
        .min_h_0()
        .px(px(24.))
        .py(px(26.))
        .child(caps("空的项目目录", 11., FontWeight::NORMAL, muted(), 0.2))
        .child(
            txt(
                "建一个文件夹，放一份 project.toml（下面这份是模板）和它指向的脚本。",
                12.,
                FontWeight::NORMAL,
                dim(),
            )
            .line_height(px(20.))
            .mt(px(10.))
            .mb(px(18.)),
        )
        .child(
            div()
                .border(px(1.))
                .border_color(line())
                .p(px(16.))
                .child(template),
        )
        .when_some(state.projects_error.clone(), |this, error| {
            this.child(
                txt(error, 11., FontWeight::NORMAL, warn())
                    .mt(px(14.))
                    .line_height(px(18.)),
            )
        })
}

fn script_body(project: &Project) -> AnyElement {
    match (&project.body, project.readiness) {
        (Some(body), _) => v()
            .id("startup-script")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .px(px(20.))
            .py(px(16.))
            .child(
                txt(body.clone(), 11., FontWeight::NORMAL, text())
                    .font_family("Noto Sans Mono CJK SC")
                    .line_height(px(19.)),
            )
            .into_any_element(),
        (None, readiness) => v()
            .flex_1()
            .min_h_0()
            .px(px(24.))
            .py(px(28.))
            .child(caps(
                readiness.label(),
                11.,
                FontWeight::NORMAL,
                readiness_color(readiness),
                0.2,
            ))
            .child(
                txt(
                    project.problem.clone().unwrap_or_else(|| {
                        format!("脚本 {} 不存在或不可读。", project.path().display())
                    }),
                    12.,
                    FontWeight::NORMAL,
                    dim(),
                )
                .line_height(px(20.))
                .mt(px(10.))
                .max_w(px(520.)),
            )
            .into_any_element(),
    }
}

fn project_inspector_pane(overview: &Overview, weak: &WeakEntity<App>) -> Div {
    let state = overview.startup;
    v().w(px(304.))
        .flex_shrink_0()
        .child(pane_head("检查器".to_string(), None))
        .child(
            v().id("startup-project-inspect")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .children(project_inspector_groups(state, weak)),
        )
}

fn project_inspector_groups(state: &StartupState, weak: &WeakEntity<App>) -> Vec<AnyElement> {
    let Some(project) = state.selected_project() else {
        return vec![
            v().p(px(16.))
                .child(caps("未选择项目", 10., FontWeight::NORMAL, dim(), 0.2))
                .child(
                    txt(
                        "左侧选中一个项目后，这里显示它的清单字段与启动命令。",
                        11.5,
                        FontWeight::NORMAL,
                        dim(),
                    )
                    .line_height(px(19.))
                    .mt(px(8.)),
                )
                .into_any_element(),
        ];
    };

    let run_weak = weak.clone();
    let command = project.command();
    vec![
        v().p(px(16.))
            .border_b(px(1.))
            .border_color(line())
            .child(
                h().items_center()
                    .justify_between()
                    .child(caps("项目", 10., FontWeight::NORMAL, dim(), 0.2))
                    .child(
                        h().items_center()
                            .gap(px(6.))
                            .child(
                                div()
                                    .size(px(5.))
                                    .rounded_full()
                                    .bg(readiness_color(project.readiness)),
                            )
                            .child(caps(
                                project.readiness.label(),
                                9.5,
                                FontWeight::NORMAL,
                                dim(),
                                0.14,
                            )),
                    ),
            )
            .child(
                div()
                    .mt(px(10.))
                    .child(field("名称", project.name.clone(), false)),
            )
            .child(field("目录", project.id.clone(), true))
            .into_any_element(),
        v().p(px(16.))
            .border_b(px(1.))
            .border_color(line())
            .child(caps(
                "清单 · project.toml",
                10.,
                FontWeight::NORMAL,
                dim(),
                0.2,
            ))
            .child(
                div()
                    .mt(px(8.))
                    .child(field("入口脚本", project.script.clone(), true)),
            )
            .child(field("解释器", project.interpreter.clone(), true))
            .child(field(
                "参数",
                if project.args.is_empty() {
                    "—".to_string()
                } else {
                    project.args.join(" ")
                },
                true,
            ))
            .child(field(
                "工作目录",
                project.workdir().display().to_string(),
                true,
            ))
            .child(field(
                "随开机启动",
                if project.boot {
                    "是".to_string()
                } else {
                    "否".to_string()
                },
                false,
            ))
            .child(field("开机顺序", project.order.to_string(), false))
            .into_any_element(),
        v().p(px(16.))
            .child(caps("运行", 10., FontWeight::NORMAL, dim(), 0.2))
            .child(
                txt(command, 10.5, FontWeight::NORMAL, muted())
                    .font_family("Noto Sans Mono CJK SC")
                    .line_height(px(18.))
                    .mt(px(10.))
                    .max_w(px(272.)),
            )
            .child(
                div()
                    .id("startup-project-run")
                    .mt(px(14.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .py(px(10.))
                    .border(px(1.))
                    .border_color(line_strong())
                    .cursor_pointer()
                    .hover(|this| this.bg(alpha(0xffffff14)))
                    .on_click(move |_, _, cx| {
                        let _ =
                            run_weak.update(cx, |app, cx| app.startup_run_project_unavailable(cx));
                    })
                    .child(caps("运行 · 未接入", 10.5, FontWeight::NORMAL, dim(), 0.16)),
            )
            .into_any_element(),
    ]
}

fn projects_footer() -> Div {
    h().w_full()
        .items_start()
        .justify_between()
        .gap(px(30.))
        .border_t(px(1.))
        .border_color(line())
        .mt(px(34.))
        .pt(px(22.))
        .child(
            txt(
                "项目是磁盘上的真实文件：目录里有什么就列什么，清单解析失败会标红并给出解析器原话。这一版不做执行，也不替你写脚本。",
                12.,
                FontWeight::NORMAL,
                dim(),
            )
            .line_height(px(22.)),
        )
        .child(
            div()
                .flex_shrink_0()
                .border(px(1.))
                .border_color(line())
                .px(px(10.))
                .py(px(5.))
                .child(caps("TOML · 项目自带", 10., FontWeight::NORMAL, muted(), 0.1)),
        )
}

// ============================================================================
// Journal view
// ============================================================================

fn journal(overview: &Overview, weak: &WeakEntity<App>) -> Div {
    let state = overview.startup;
    let base = startup::State::path()
        .parent()
        .map(|dir| dir.join(startup::JOURNAL))
        .unwrap_or_else(|| startup::JOURNAL.into());
    v().child(hero(
        "Startup · 尚未执行",
        "启动日志",
        "每次登录的真实执行记录：哪页成功、哪页跳过了、卡在哪个窗口。",
        weak,
    ))
        .child(status_line(state))
        .child(
            h().items_stretch()
                .border(px(1.))
                .border_color(line())
                .child(
                    v().flex_1()
                        .min_w_0()
                        .px(px(24.))
                        .py(px(28.))
                        .child(caps("暂无执行记录", 11., gpui_kit::FontWeight::NORMAL, muted(), 0.2))
                        .child(
                            txt(
                                "执行器（真正开窗、定位、切页的那一层）尚未实现，所以这里没有可显示的时间线。等它跑起来，每次登录的成功/跳过/失败会追加到下面这个文件。",
                                12.,
                                FontWeight::NORMAL,
                                dim(),
                            )
                            .line_height(px(20.))
                            .mt(px(12.))
                            .max_w(px(560.)),
                        )
                        .child(field("记录文件", base.display().to_string(), true).mt(px(20.))),
                )
                .child(
                    v().w(px(320.))
                        .flex_shrink_0()
                        .p(px(24.))
                        .border_l(px(1.))
                        .border_color(line())
                        .child(caps("能证明的事", 11., FontWeight::MEDIUM, muted(), 0.22))
                        .child(
                            field(
                                "参与页面",
                                format!(
                                    "{} / {}",
                                    state.boot_pages(),
                                    state.layout.pages.len()
                                ),
                                false,
                            )
                            .mt(px(10.)),
                        )
                        .child(field("窗口", format!("{}", state.boot_windows()), false))
                        .child(field(
                            "命令可解析",
                            match &state.preview {
                                Some(preview) if preview.miss_count() == 0 => {
                                    format!(
                                        "{} 个命令全部可执行",
                                        preview.ok_count()
                                    )
                                }
                                Some(preview) => format!("缺 {} 个", preview.miss_count()),
                                None => "未预演".to_string(),
                            },
                            false,
                        ))
                        .child(field("显示器", state.displays.len().to_string(), false)),
                ),
        )
        .child(footer())
}
