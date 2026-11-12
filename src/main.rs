mod ext;
mod icons;
mod sys;
mod theme;
mod toggles;

use std::time::{Duration, Instant};

use ext::Provider as _;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AppContext as _, Bounds, Context, Div, FontWeight, Hsla, InteractiveElement as _, IntoElement,
    MouseButton, ParentElement as _, PathBuilder, Point, Render, ResizeEdge, Stateful,
    StatefulInteractiveElement as _, Styled, TitlebarOptions, WeakEntity, Window, WindowBounds,
    WindowDecorations, WindowOptions, canvas, div, point, px, relative, rgba, size,
};
use theme::{
    FONT, accent, accent_hover, alpha, black, caps, dim, h, line, line_faint, line_strong, muted,
    ok, surface_2, text, tracked, txt, v, warn, white,
};

// ============================================================================
// App
// ============================================================================

/// Window chrome state: press tracking for drag + double-click-to-zoom.
const DOUBLE_CLICK: Duration = Duration::from_millis(320);
const DRAG_THRESHOLD: f32 = 4.;

struct App {
    probe: sys::Probe,
    snap: sys::Snapshot,
    provider: ext::FilesystemProvider,
    extensions: Vec<ext::Extension>,
    toggles: toggles::Toggles,
    switches: Vec<toggles::Toggle>,
    /// UUID of the extension whose details are open in the right panel.
    selected: Option<String>,
    /// Last provider failure, shown inside the detail panel.
    error: Option<String>,
    nav: Nav,
    last_click: Option<Instant>,
    drag_anchor: Option<Point<gpui_kit::Pixels>>,
}

/// How often the overview re-samples the machine.
const REFRESH: Duration = Duration::from_secs(1);
/// Extensions change rarely; re-reading gsettings every tick would be waste.
const EXTENSION_TICKS: u32 = 10;

impl App {
    fn new(cx: &mut Context<Self>) -> Self {
        let mut probe = sys::Probe::new();
        let snap = probe.sample();
        let provider = ext::FilesystemProvider::discover();
        let extensions = provider.list();
        let toggles = toggles::Toggles::new();
        let switches = toggles.read();
        // Sampling runs on the UI thread: every probe is a procfs/sysfs read,
        // so a tick costs microseconds and never races the render.
        cx.spawn(async move |this, cx| {
            let mut ticks = 0u32;
            loop {
                cx.background_executor().timer(REFRESH).await;
                ticks = ticks.wrapping_add(1);
                let rescan = ticks % EXTENSION_TICKS == 0;
                let alive = this
                    .update(cx, |app, cx| {
                        app.snap = app.probe.sample();
                        if rescan {
                            app.provider.reload();
                            app.extensions = app.provider.list();
                            app.switches = app.toggles.read();
                        }
                        cx.notify();
                    })
                    .is_ok();
                if !alive {
                    break;
                }
            }
        })
        .detach();
        Self {
            probe,
            snap,
            provider,
            extensions,
            toggles,
            switches,
            selected: None,
            error: None,
            nav: Nav::default(),
            last_click: None,
            drag_anchor: None,
        }
    }

    /// Switch section: the rail highlight and the tab row both follow.
    fn go_section(&mut self, section: Section, cx: &mut Context<Self>) {
        self.nav = Nav {
            section,
            page: section.pages()[0],
        };
        self.selected = None;
        self.error = None;
        cx.notify();
    }

    fn go_page(&mut self, page: Page, cx: &mut Context<Self>) {
        self.nav.page = page;
        self.selected = None;
        self.error = None;
        cx.notify();
    }

    /// Open the detail panel for `uuid`, or close it when it is already open.
    fn select_extension(&mut self, uuid: &str, cx: &mut Context<Self>) {
        self.selected = if self.selected.as_deref() == Some(uuid) {
            None
        } else {
            Some(uuid.to_string())
        };
        self.error = None;
        cx.notify();
    }

    /// Flip a right-column switch and re-read all of them, so the UI shows
    /// what the system accepted rather than what we asked for.
    fn toggle_switch(&mut self, id: &str, on: bool, cx: &mut Context<Self>) {
        self.error = self.toggles.set(id, on).err();
        self.switches = self.toggles.read();
        cx.notify();
    }

    fn clear_selection(&mut self, cx: &mut Context<Self>) {
        self.selected = None;
        self.error = None;
        cx.notify();
    }

    /// Flip an extension through the active provider, then re-read the
    /// inventory so the row reflects what the system actually accepted.
    fn toggle_extension(&mut self, uuid: &str, enabled: bool, cx: &mut Context<Self>) {
        self.error = match self.provider.set_enabled(uuid, enabled) {
            Ok(()) => None,
            Err(error) => Some(error),
        };
        self.provider.reload();
        self.extensions = self.provider.list();
        cx.notify();
    }
}

impl Render for App {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let weak = cx.entity().downgrade();
        h().relative()
            .size_full()
            .items_stretch()
            .bg(black())
            .font_family(FONT)
            .text_color(text())
            .child(rail(self.nav.section, &weak))
            .child(
                v().flex_1()
                    .min_w_0()
                    .min_h_0()
                    .child(self.topbar(cx))
                    .child(content(
                        &Overview {
                            snap: &self.snap,
                            extensions: &self.extensions,
                            switches: &self.switches,
                            selected: self.selected.as_deref(),
                            error: self.error.as_deref(),
                            page: self.nav.page,
                        },
                        cx,
                    )),
            )
            .child(resize_grips())
    }
}

// ============================================================================
// Navigation: rail selects a section, the top bar tabs select a page inside it
// ============================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Section {
    Overview,
    Extensions,
    Appearance,
    Devices,
    Power,
}

impl Section {
    const ALL: &'static [Section] = &[
        Section::Overview,
        Section::Extensions,
        Section::Appearance,
        Section::Devices,
        Section::Power,
    ];

    fn index(self) -> u32 {
        match self {
            Section::Overview => 0,
            Section::Extensions => 1,
            Section::Appearance => 2,
            Section::Devices => 3,
            Section::Power => 4,
        }
    }

    fn icon(self) -> &'static str {
        match self {
            Section::Overview => icons::GRID,
            Section::Extensions => icons::PUZZLE,
            Section::Appearance => icons::SUN,
            Section::Devices => icons::MONITOR,
            Section::Power => icons::POWER,
        }
    }

    /// Pages this section shows in the top bar. First one is the default.
    fn pages(self) -> &'static [Page] {
        match self {
            Section::Overview => &[Page::Overview, Page::Processes],
            Section::Extensions => &[Page::Installed],
            Section::Appearance => &[Page::Appearance],
            Section::Devices => &[Page::Devices],
            Section::Power => &[Page::Power],
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Page {
    Overview,
    Processes,
    Installed,
    Appearance,
    Devices,
    Power,
}

impl Page {
    fn index(self) -> u32 {
        match self {
            Page::Overview => 0,
            Page::Processes => 1,
            Page::Installed => 2,
            Page::Appearance => 3,
            Page::Devices => 4,
            Page::Power => 5,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Page::Overview => "Overview",
            Page::Processes => "Processes",
            Page::Installed => "Installed",
            Page::Appearance => "Appearance",
            Page::Devices => "Devices",
            Page::Power => "Power",
        }
    }
}

/// Single source of truth for navigation: both the rail and the tabs render
/// from it, so they can never disagree.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Nav {
    section: Section,
    page: Page,
}

impl Default for Nav {
    fn default() -> Self {
        Self {
            section: Section::Overview,
            page: Page::Overview,
        }
    }
}

// ============================================================================
// Left icon rail
// ============================================================================

fn rail(current: Section, weak: &WeakEntity<App>) -> Div {
    let mut column = v()
        .w(px(68.))
        .flex_shrink_0()
        .items_center()
        .py(px(22.))
        .gap(px(6.))
        .border_r(px(1.))
        .border_color(line())
        .child(
            div()
                .size(px(30.))
                .mb(px(26.))
                .flex()
                .items_center()
                .justify_center()
                .border(px(1.))
                .border_color(alpha(0xffffff2e))
                .child(tracked("G", 11., FontWeight::SEMIBOLD, text(), 0.08)),
        );
    for section in Section::ALL {
        column = column.child(rail_button(*section, *section == current, weak));
    }
    column.child(div().flex_1()).child(rail_settings_button())
}

fn rail_button(section: Section, active: bool, weak: &WeakEntity<App>) -> Stateful<Div> {
    let color = if active { text() } else { dim() };
    let weak = weak.clone();
    div()
        .relative()
        .w(px(40.))
        .h(px(40.))
        .flex()
        .items_center()
        .justify_center()
        .id(("rail", section.index()))
        .cursor_pointer()
        .hover(|this| this.bg(alpha(0xffffff0a)))
        .when(active, |this| {
            this.child(
                div()
                    .absolute()
                    .left(px(-14.))
                    .top(px(12.))
                    .bottom(px(12.))
                    .w(px(2.))
                    .bg(accent()),
            )
        })
        .child(icons::icon(section.icon(), 18., color))
        .on_click(move |_, _, cx| {
            let _ = weak.update(cx, |app, cx| app.go_section(section, cx));
        })
}

/// Settings is not a section yet; keep the icon but leave it inert.
fn rail_settings_button() -> impl IntoElement {
    div()
        .w(px(40.))
        .h(px(40.))
        .flex()
        .items_center()
        .justify_center()
        .child(icons::icon(icons::SETTINGS, 18., dim()))
}

// ============================================================================
// Top bar
// ============================================================================

impl App {
    /// The 56px header doubles as the window titlebar: drag to move the window,
    /// double-click to zoom (maximize/restore), right-click for the native
    /// GNOME window menu.
    fn topbar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let weak_down = cx.entity().downgrade();
        let weak_move = cx.entity().downgrade();
        let weak_up = cx.entity().downgrade();
        let tab_weak = cx.entity().downgrade();
        let snap = &self.snap;

        h().id("titlebar")
            .h(px(56.))
            .flex_shrink_0()
            .items_center()
            .gap(px(34.))
            .px(px(40.))
            .border_b(px(1.))
            .border_color(line())
            .child(tracked("GNOUS", 13., FontWeight::MEDIUM, text(), 0.34))
            .child(
                h().h_full().items_stretch().gap(px(30.)).children(
                    self.nav
                        .section
                        .pages()
                        .iter()
                        .map(|page| nav_item(*page, *page == self.nav.page, &tab_weak)),
                ),
            )
            .child(div().flex_1())
            .child(
                h().items_center()
                    .gap(px(22.))
                    .text_size(px(12.))
                    .text_color(muted())
                    .child(
                        h().items_center()
                            .gap(px(7.))
                            .child(div().size(px(5.)).rounded_full().bg(if snap.healthy {
                                ok()
                            } else {
                                warn()
                            }))
                            .child(div().child(if snap.healthy {
                                "System nominal"
                            } else {
                                "System attention"
                            })),
                    )
                    .child(div().child(snap.session.clone()))
                    .child(div().text_color(text()).child(snap.clock.clone())),
            )
            .on_mouse_down(MouseButton::Left, move |event, window, cx| {
                let now = Instant::now();
                let mut zoom = false;
                let _ = weak_down.update(cx, |this, _| {
                    let double = this
                        .last_click
                        .is_some_and(|at| now.duration_since(at) < DOUBLE_CLICK);
                    if double {
                        this.last_click = None;
                        this.drag_anchor = None;
                        zoom = true;
                    } else {
                        this.last_click = Some(now);
                        this.drag_anchor = Some(event.position);
                    }
                });
                if zoom {
                    window.zoom_window();
                }
            })
            .on_mouse_move(move |event, window, cx| {
                if event.pressed_button != Some(MouseButton::Left) {
                    return;
                }
                let mut begin_move = false;
                let _ = weak_move.update(cx, |this, _| {
                    if let Some(anchor) = this.drag_anchor {
                        let moved = (event.position.x.as_f32() - anchor.x.as_f32()).abs()
                            > DRAG_THRESHOLD
                            || (event.position.y.as_f32() - anchor.y.as_f32()).abs()
                                > DRAG_THRESHOLD;
                        if moved {
                            this.drag_anchor = None;
                            begin_move = true;
                        }
                    }
                });
                if begin_move {
                    window.start_window_move();
                }
            })
            .on_mouse_up(MouseButton::Left, move |_, _, cx| {
                let _ = weak_up.update(cx, |this, _| this.drag_anchor = None);
            })
            .on_mouse_down(MouseButton::Right, |event, window, _| {
                window.show_window_menu(event.position);
            })
    }
}

/// Invisible resize grips for the client-side-decorated window edges.
fn resize_grips() -> impl IntoElement {
    div()
        .absolute()
        .top_0()
        .left_0()
        .right_0()
        .bottom_0()
        .child(grip("rz-top", ResizeEdge::Top, |d| {
            d.top_0().left(px(12.)).right(px(12.)).h(px(5.))
        }))
        .child(grip("rz-bottom", ResizeEdge::Bottom, |d| {
            d.bottom_0().left(px(12.)).right(px(12.)).h(px(5.))
        }))
        .child(grip("rz-left", ResizeEdge::Left, |d| {
            d.left_0().top(px(12.)).bottom(px(12.)).w(px(5.))
        }))
        .child(grip("rz-right", ResizeEdge::Right, |d| {
            d.right_0().top(px(12.)).bottom(px(12.)).w(px(5.))
        }))
        .child(grip("rz-top-left", ResizeEdge::TopLeft, |d| {
            d.top_0().left_0().size(px(12.))
        }))
        .child(grip("rz-top-right", ResizeEdge::TopRight, |d| {
            d.top_0().right_0().size(px(12.))
        }))
        .child(grip("rz-bottom-left", ResizeEdge::BottomLeft, |d| {
            d.bottom_0().left_0().size(px(12.))
        }))
        .child(grip("rz-bottom-right", ResizeEdge::BottomRight, |d| {
            d.bottom_0().right_0().size(px(12.))
        }))
}

fn grip(id: &'static str, edge: ResizeEdge, place: impl FnOnce(Div) -> Div) -> impl IntoElement {
    place(div().absolute())
        .id(id)
        .on_mouse_down(MouseButton::Left, move |_, window, _| {
            window.start_window_resize(edge);
        })
}

fn nav_item(page: Page, active: bool, weak: &WeakEntity<App>) -> Stateful<Div> {
    let color = if active { text() } else { muted() };
    let weakness = weak.clone();
    div()
        .h_full()
        .flex()
        .items_center()
        .id(("nav", page.index()))
        .cursor_pointer()
        .border_b(px(1.))
        .border_color(if active { accent() } else { alpha(0x00000000) })
        .hover(|this| this.border_color(if active { accent() } else { line_strong() }))
        .child(caps(page.label(), 12., FontWeight::NORMAL, color, 0.13))
        .on_click(move |_, _, cx| {
            let _ = weakness.update(cx, |app, cx| app.go_page(page, cx));
        })
}

// ============================================================================
// Scrollable content
// ============================================================================

/// Everything the overview needs for one frame, so layout helpers take one
/// argument instead of six.
struct Overview<'a> {
    snap: &'a sys::Snapshot,
    extensions: &'a [ext::Extension],
    switches: &'a [toggles::Toggle],
    selected: Option<&'a str>,
    error: Option<&'a str>,
    page: Page,
}

fn content(overview: &Overview, cx: &mut Context<App>) -> impl IntoElement {
    let weak = cx.entity().downgrade();
    let body = match overview.page {
        Page::Overview => overview_body(overview, &weak),
        Page::Installed => extensions_body(overview, &weak),
        page => placeholder(page),
    };
    v().id("content")
        .flex_1()
        .min_h_0()
        .overflow_y_scroll()
        .pl(px(40.))
        .pr(px(52.))
        .pt(px(52.))
        .pb(px(64.))
        .child(body)
}

fn overview_body(overview: &Overview, weak: &WeakEntity<App>) -> Div {
    v().child(hero(overview.snap))
        .child(metrics(overview.snap))
        .child(section_head(
            "扩展与开关",
            &format!("管理全部 {} 项 →", overview.extensions.len()),
        ))
        .child(panels(overview, weak))
        .child(section_head("资源热点", "打开任务管理器 →"))
        .child(processes())
        .child(section_head("启动器 · 常用", "编辑布局 →"))
        .child(apps())
}

/// Extensions section: the same panel, without the overview chrome.
fn extensions_body(overview: &Overview, weak: &WeakEntity<App>) -> Div {
    v().child(section_head(
        "扩展与开关",
        &format!("管理全部 {} 项 →", overview.extensions.len()),
    ))
    .child(panels(overview, weak))
}

/// Sections whose content is not designed yet.
fn placeholder(page: Page) -> Div {
    v().pt(px(30.))
        .pb(px(80.))
        .child(caps("待设计", 10.5, FontWeight::NORMAL, dim(), 0.22))
        .child(
            txt(page.label(), 44., FontWeight::EXTRA_LIGHT, text())
                .line_height(px(46.))
                .mt(px(14.)),
        )
        .child(
            txt(
                "这个页面的内容还没实现，下一步先出 HTML demo 选型。",
                13.,
                FontWeight::NORMAL,
                muted(),
            )
            .mt(px(12.)),
        )
}

fn hero(snap: &sys::Snapshot) -> Div {
    h().items_end()
        .justify_between()
        .mb(px(56.))
        .child(
            v().child(
                caps(
                    &format!("Session · {} · {}", snap.hostname, snap.session_kind),
                    11.,
                    FontWeight::NORMAL,
                    dim(),
                    0.26,
                )
                .mb(px(14.)),
            )
            .child(txt(snap.headline, 44., FontWeight::EXTRA_LIGHT, text()).line_height(px(46.)))
            .child(txt(snap.detail.clone(), 13., FontWeight::NORMAL, muted()).mt(px(12.))),
        )
        .child(
            h().items_center()
                .gap(px(10.))
                .child(button("扫描更新", false))
                .child(button("重启会话", true)),
        )
}

fn button(label: &'static str, solid: bool) -> Stateful<Div> {
    let (bg, border, fg) = if solid {
        (accent(), accent(), white())
    } else {
        (alpha(0x00000000), line_strong(), text())
    };
    let (hover_bg, hover_border, hover_fg) = if solid {
        (accent_hover(), accent_hover(), white())
    } else {
        (white(), white(), black())
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
        .hover(move |this| {
            this.bg(hover_bg)
                .border_color(hover_border)
                .text_color(hover_fg)
        })
        .child(caps_no_color(label, 11., FontWeight::NORMAL, 0.16))
}

// ============================================================================
// Metric row
// ============================================================================

struct Metric {
    label: &'static str,
    value: String,
    unit: String,
    fill: f32,
    hot: bool,
    left: String,
    right: String,
}

fn metrics(snap: &sys::Snapshot) -> Div {
    h().items_stretch()
        .border_t(px(1.))
        .border_color(line())
        .mb(px(58.))
        .child(metric(
            0,
            Metric {
                label: "CPU 负载",
                value: snap.cpu_value.clone(),
                unit: "%".into(),
                fill: snap.cpu_fill,
                hot: snap.cpu_fill >= 0.85,
                left: snap.cpu_left.clone(),
                right: snap.cpu_right.clone(),
            },
        ))
        .child(metric(
            1,
            Metric {
                label: "内存",
                value: snap.mem_value.clone(),
                unit: snap.mem_unit.clone(),
                fill: snap.mem_fill,
                hot: snap.mem_fill >= 0.85,
                left: snap.mem_left.clone(),
                right: snap.mem_right.clone(),
            },
        ))
        .child(metric(
            2,
            Metric {
                label: "存储 · 根分区",
                value: snap.disk_value.clone(),
                unit: snap.disk_unit.clone(),
                fill: snap.disk_fill,
                hot: snap.disk_fill >= 0.8,
                left: snap.disk_left.clone(),
                right: snap.disk_right.clone(),
            },
        ))
        .child(metric(
            3,
            Metric {
                label: "网络",
                value: snap.net_value.clone(),
                unit: snap.net_unit.clone(),
                fill: snap.net_fill,
                hot: false,
                left: snap.net_left.clone(),
                right: snap.net_right.clone(),
            },
        ))
}

fn metric(index: usize, m: Metric) -> Div {
    v().w(relative(0.25))
        .min_w_0()
        .pt(px(30.))
        .pb(px(26.))
        .pr(px(28.))
        .when(index > 0, |this| {
            this.pl(px(28.)).border_l(px(1.)).border_color(line())
        })
        .child(caps(m.label, 10.5, FontWeight::NORMAL, dim(), 0.22).mb(px(22.)))
        .child(
            h().items_end()
                .gap(px(8.))
                .child(txt(m.value, 52., FontWeight::EXTRA_LIGHT, text()).line_height(px(52.)))
                .child(
                    txt(m.unit, 15., FontWeight::LIGHT, muted())
                        .line_height(px(18.))
                        .mb(px(4.)),
                ),
        )
        .child(
            div()
                .h(px(1.))
                .w_full()
                .mt(px(24.))
                .bg(line_strong())
                .child(div().h_full().w(relative(m.fill)).bg(if m.hot {
                    accent()
                } else {
                    text()
                })),
        )
        .child(
            h().justify_between()
                .mt(px(11.))
                .text_size(px(10.5))
                .text_color(dim())
                .child(txt(m.left, 10.5, FontWeight::NORMAL, dim()))
                .child(txt(m.right, 10.5, FontWeight::NORMAL, dim())),
        )
}

// ============================================================================
// Section header
// ============================================================================

fn section_head(title: &str, link: &str) -> Div {
    h().items_end()
        .justify_between()
        .mb(px(22.))
        .child(caps(title, 12., FontWeight::MEDIUM, muted(), 0.24))
        .child(caps(link, 11., FontWeight::NORMAL, dim(), 0.12))
}

// ============================================================================
// Extensions + quick toggles
// ============================================================================

fn panels(overview: &Overview, weak: &WeakEntity<App>) -> Div {
    let detail = overview
        .selected
        .and_then(|uuid| overview.extensions.iter().find(|e| e.uuid == uuid));
    h().items_stretch()
        .gap(px(1.))
        .bg(line())
        .border(px(1.))
        .border_color(line())
        .mb(px(58.))
        .child(
            v().w(relative(0.574))
                .min_w_0()
                .bg(black())
                .pt(px(30.))
                .pb(px(30.))
                .px(px(28.))
                .children(ext_rows(overview.extensions, overview.selected, weak)),
        )
        .child(match detail {
            Some(extension) => details(extension, overview.error, weak),
            None => quick_toggles(overview, weak),
        })
}

/// Right-hand panel when nothing is selected: the real system switches.
fn quick_toggles(overview: &Overview, weak: &WeakEntity<App>) -> Div {
    let last = overview.switches.len().saturating_sub(1);
    v().flex_1()
        .min_w_0()
        .bg(black())
        .pt(px(30.))
        .pb(px(30.))
        .px(px(28.))
        .children(
            overview
                .switches
                .iter()
                .enumerate()
                .map(|(index, toggle)| toggle_row(index, toggle, index == last, weak)),
        )
        .when_some(overview.error, |this, error| {
            this.child(txt(error.to_string(), 10.5, FontWeight::NORMAL, warn()).mt(px(14.)))
        })
}

fn toggle_row(
    index: usize,
    toggle: &toggles::Toggle,
    last: bool,
    weak: &WeakEntity<App>,
) -> Stateful<Div> {
    let (on, available, id) = (toggle.on, toggle.available, toggle.id);
    let title_color = if available { text() } else { muted() };
    let weak = weak.clone();
    h().items_center()
        .id(("toggle", index))
        .justify_between()
        .py(px(16.))
        .when(!last, |this| this.border_b(px(1.)).border_color(line()))
        .when(available, |this| {
            this.cursor_pointer()
                .hover(|this| this.bg(alpha(0xffffff0a)))
                .on_click(move |_, _, cx| {
                    let _ = weak.update(cx, |app, cx| app.toggle_switch(id, !on, cx));
                })
        })
        .child(
            v().child(txt(toggle.title, 13., FontWeight::NORMAL, title_color))
                .child(txt(toggle.description.clone(), 11., FontWeight::NORMAL, dim()).mt(px(4.))),
        )
        .child(switch(on, available))
}

fn switch(on: bool, available: bool) -> impl IntoElement {
    let border = if !available {
        line()
    } else if on {
        alpha(0xffffff8c)
    } else {
        line_strong()
    };
    let fill = if !available {
        alpha(0xffffff03)
    } else if on {
        alpha(0xffffff14)
    } else {
        alpha(0xffffff05)
    };
    let knob = if !available {
        dim()
    } else if on {
        text()
    } else {
        muted()
    };
    div()
        .relative()
        .w(px(46.))
        .h(px(22.))
        .flex_shrink_0()
        .rounded_full()
        .border(px(1.))
        .border_color(border)
        .bg(fill)
        .child(
            div()
                .absolute()
                .top(px(3.))
                .left(px(if on { 27. } else { 3. }))
                .w(px(14.))
                .h(px(14.))
                .rounded_full()
                .bg(knob),
        )
}

/// Detail panel: what the extension is, where it came from, and its switch.
fn details(extension: &ext::Extension, error: Option<&str>, weak: &WeakEntity<App>) -> Div {
    let (_, enabled) = extension.status();
    let (uuid, want) = (extension.uuid.clone(), !enabled);
    let toggle_weak = weak.clone();
    let close_weak = weak.clone();
    let description = if extension.description.trim().is_empty() {
        "这个扩展没有提供描述。".to_string()
    } else {
        extension.description.clone()
    };
    let shell_versions = if extension.shell_versions.is_empty() {
        "—".to_string()
    } else {
        extension.shell_versions.join(" · ")
    };
    let version = if extension.version.is_empty() {
        "—".to_string()
    } else {
        format!("v{}", extension.version)
    };
    let preferences = if extension.has_prefs { "有" } else { "无" };

    v().flex_1()
        .min_w_0()
        .bg(black())
        .pt(px(30.))
        .pb(px(30.))
        .px(px(28.))
        .child(
            h().items_center()
                .justify_between()
                .child(caps("扩展详情", 10.5, FontWeight::NORMAL, dim(), 0.22))
                .child(
                    div()
                        .id("detail-close")
                        .cursor_pointer()
                        .child(caps("关闭", 10., FontWeight::NORMAL, dim(), 0.14))
                        .hover(|this| this.text_color(text()))
                        .on_click(move |_, _, cx| {
                            let _ = close_weak.update(cx, |app, cx| app.clear_selection(cx));
                        }),
                ),
        )
        .child(
            txt(extension.name.clone(), 21., FontWeight::LIGHT, text())
                .line_height(px(26.))
                .mt(px(16.)),
        )
        .child(txt(extension.uuid.clone(), 10.5, FontWeight::NORMAL, dim()).mt(px(4.)))
        .child(div().h(px(1.)).w_full().bg(line()).mt(px(18.)).mb(px(18.)))
        .child(
            txt(description, 12., FontWeight::NORMAL, muted())
                .line_height(px(19.))
                .mb(px(18.)),
        )
        .child(info_row("版本", version))
        .child(info_row("来源", extension.origin.label().to_string()))
        .child(info_row("支持 GNOME", shell_versions))
        .child(info_row("首选项", preferences.to_string()))
        .when_some(error, |this, error| {
            this.child(txt(error.to_string(), 10.5, FontWeight::NORMAL, warn()).mt(px(14.)))
        })
        .child(
            button(
                if enabled {
                    "禁用扩展"
                } else {
                    "启用扩展"
                },
                !enabled,
            )
            .mt(px(24.))
            .on_click(move |_, _, cx| {
                let uuid = uuid.clone();
                let _ = toggle_weak.update(cx, |app, cx| {
                    app.toggle_extension(&uuid, want, cx);
                });
            }),
        )
}

fn info_row(label: &str, value: String) -> Div {
    h().items_center()
        .justify_between()
        .py(px(8.))
        .border_b(px(1.))
        .border_color(line_faint())
        .child(caps(label, 9.5, FontWeight::NORMAL, dim(), 0.2))
        .child(txt(value, 11.5, FontWeight::NORMAL, text()))
}

/// Rows for the extension panel, or a hint when the machine has none.
fn ext_rows(
    extensions: &[ext::Extension],
    selected: Option<&str>,
    weak: &WeakEntity<App>,
) -> Vec<Stateful<Div>> {
    if extensions.is_empty() {
        return vec![empty_extensions()];
    }
    let last = extensions.len() - 1;
    extensions
        .iter()
        .enumerate()
        .map(|(index, extension)| {
            let open = selected == Some(extension.uuid.as_str());
            ext_row(index, index == last, extension, open, weak)
        })
        .collect()
}

fn empty_extensions() -> Stateful<Div> {
    let dir = std::env::var("HOME")
        .map(|home| format!("{home}/.local/share/gnome-shell/extensions"))
        .unwrap_or_else(|_| "~/.local/share/gnome-shell/extensions".into());
    v().id("ext-empty")
        .py(px(24.))
        .child(txt("未检测到已安装扩展", 13., FontWeight::NORMAL, muted()))
        .child(
            txt(
                format!("放入 {dir} 后 10 秒内自动出现"),
                10.5,
                FontWeight::NORMAL,
                dim(),
            )
            .mt(px(6.)),
        )
}

fn ext_row(
    index: usize,
    last: bool,
    extension: &ext::Extension,
    open: bool,
    weak: &WeakEntity<App>,
) -> Stateful<Div> {
    let (status, enabled) = extension.status();
    let status_color = if enabled { ok() } else { dim() };
    let uuid = extension.uuid.clone();
    let weak = weak.clone();
    h().relative()
        .items_center()
        .id(("ext", index))
        .cursor_pointer()
        .gap(px(16.))
        .py(px(15.))
        .when(open, |this| {
            this.bg(alpha(0xffffff0a)).child(
                div()
                    .absolute()
                    .left(px(-28.))
                    .top(px(10.))
                    .bottom(px(10.))
                    .w(px(2.))
                    .bg(accent()),
            )
        })
        .hover(|this| this.bg(surface_2()))
        .on_click(move |_, _, cx| {
            let uuid = uuid.clone();
            let _ = weak.update(cx, |app, cx| app.select_extension(&uuid, cx));
        })
        .when(!last, |this| this.border_b(px(1.)).border_color(line()))
        .child(
            div()
                .w(px(30.))
                .h(px(30.))
                .flex_shrink_0()
                .flex()
                .items_center()
                .justify_center()
                .border(px(1.))
                .border_color(line())
                .child(icons::icon(icons::PUZZLE, 14., muted())),
        )
        .child(
            v().flex_1()
                .min_w_0()
                .child(txt(extension.name.clone(), 13., FontWeight::NORMAL, text()))
                .child(
                    txt(
                        format!("{} · {}", extension.meta_line(), extension.origin.label()),
                        10.5,
                        FontWeight::NORMAL,
                        dim(),
                    )
                    .mt(px(3.)),
                ),
        )
        .child(caps(status, 9.5, FontWeight::NORMAL, status_color, 0.18))
}

// ============================================================================
// Process table
// ============================================================================

fn processes() -> Div {
    v().border(px(1.))
        .border_color(line())
        .pt(px(26.))
        .pb(px(26.))
        .px(px(28.))
        .mb(px(58.))
        .child(
            h().items_end()
                .pb(px(14.))
                .border_b(px(1.))
                .border_color(line())
                .child(
                    div()
                        .flex_1()
                        .child(caps("进程", 10., FontWeight::MEDIUM, dim(), 0.2)),
                )
                .child(
                    div()
                        .w(px(160.))
                        .child(caps("状态", 10., FontWeight::MEDIUM, dim(), 0.2)),
                )
                .child(div().w(px(162.)).flex().justify_end().child(caps(
                    "CPU",
                    10.,
                    FontWeight::MEDIUM,
                    dim(),
                    0.2,
                )))
                .child(div().w(px(162.)).flex().justify_end().child(caps(
                    "内存",
                    10.,
                    FontWeight::MEDIUM,
                    dim(),
                    0.2,
                )))
                .child(div().w(px(240.)).flex().justify_end().child(caps(
                    "趋势",
                    10.,
                    FontWeight::MEDIUM,
                    dim(),
                    0.2,
                ))),
        )
        .child(process_row(
            0,
            "gnome-shell",
            "PID 1842",
            "RUNNING",
            ok(),
            "8.4%",
            "412 MB",
            SPARK_SHELL,
        ))
        .child(process_row(
            1,
            "firefox",
            "PID 9210",
            "SLEEP",
            muted(),
            "6.1%",
            "1.8 GB",
            SPARK_FIREFOX,
        ))
        .child(process_row(
            2,
            "btrfs-scrub",
            "PID 44102",
            "IO WAIT",
            accent(),
            "2.2%",
            "86 MB",
            SPARK_SCRUB,
        ))
        .child(process_row_flat(
            3,
            "pipewire",
            "PID 2103",
            "SLEEP",
            muted(),
            "0.4%",
            "62 MB",
        ))
}

fn process_row(
    index: usize,
    name: &'static str,
    pid: &'static str,
    state: &'static str,
    state_color: Hsla,
    cpu: &'static str,
    mem: &'static str,
    spark: &'static [(f32, f32)],
) -> Div {
    process_row_base(index)
        .child(name_cell(name, pid))
        .child(
            div()
                .w(px(160.))
                .child(caps(state, 10.5, FontWeight::NORMAL, state_color, 0.14)),
        )
        .child(div().w(px(162.)).flex().justify_end().child(txt(
            cpu,
            12.5,
            FontWeight::NORMAL,
            muted(),
        )))
        .child(div().w(px(162.)).flex().justify_end().child(txt(
            mem,
            12.5,
            FontWeight::NORMAL,
            muted(),
        )))
        .child(div().w(px(240.)).flex().justify_end().child(sparkline(
            spark,
            64.,
            16.,
            rgba(0xffffffb3),
        )))
}

fn process_row_flat(
    index: usize,
    name: &'static str,
    pid: &'static str,
    state: &'static str,
    state_color: Hsla,
    cpu: &'static str,
    mem: &'static str,
) -> Div {
    process_row_base(index)
        .child(name_cell(name, pid))
        .child(
            div()
                .w(px(160.))
                .child(caps(state, 10.5, FontWeight::NORMAL, state_color, 0.14)),
        )
        .child(div().w(px(162.)).flex().justify_end().child(txt(
            cpu,
            12.5,
            FontWeight::NORMAL,
            muted(),
        )))
        .child(div().w(px(162.)).flex().justify_end().child(txt(
            mem,
            12.5,
            FontWeight::NORMAL,
            muted(),
        )))
        .child(
            div()
                .w(px(240.))
                .flex()
                .justify_end()
                .child(div().w(px(64.)).h(px(1.)).bg(rgba(0xffffff59))),
        )
}

fn process_row_base(index: usize) -> Div {
    h().items_center().py(px(14.)).when(index < 3, |this| {
        this.border_b(px(1.)).border_color(line_faint())
    })
}

fn name_cell(name: &'static str, pid: &'static str) -> Div {
    h().flex_1()
        .min_w_0()
        .items_center()
        .gap(px(8.))
        .child(txt(name, 12.5, FontWeight::NORMAL, text()))
        .child(txt(pid, 10.5, FontWeight::NORMAL, dim()))
}

// ============================================================================
// Sparkline
// ============================================================================

const SPARK_SHELL: &[(f32, f32)] = &[
    (0., 12.),
    (8., 10.),
    (16., 11.),
    (24., 7.),
    (32., 9.),
    (40., 6.),
    (48., 8.),
    (56., 5.),
    (64., 6.),
];
const SPARK_FIREFOX: &[(f32, f32)] = &[
    (0., 4.),
    (8., 6.),
    (16., 3.),
    (24., 5.),
    (32., 8.),
    (40., 7.),
    (48., 9.),
    (56., 8.),
    (64., 10.),
];
const SPARK_SCRUB: &[(f32, f32)] = &[
    (0., 14.),
    (8., 13.),
    (16., 14.),
    (24., 11.),
    (32., 13.),
    (40., 10.),
    (48., 12.),
    (56., 9.),
    (64., 11.),
];

fn sparkline(
    points: &'static [(f32, f32)],
    width: f32,
    height: f32,
    color: gpui_kit::Rgba,
) -> impl IntoElement {
    canvas(
        |_, _, _| (),
        move |bounds, _, window: &mut Window, _cx: &mut gpui_kit::App| {
            let sx = bounds.size.width.as_f32() / width;
            let sy = bounds.size.height.as_f32() / height;
            let mut builder = PathBuilder::stroke(px(1.));
            for (i, (x, y)) in points.iter().enumerate() {
                let p = point(bounds.origin.x + px(x * sx), bounds.origin.y + px(y * sy));
                if i == 0 {
                    builder.move_to(p);
                } else {
                    builder.line_to(p);
                }
            }
            if let Ok(path) = builder.build() {
                window.paint_path(path, color);
            }
        },
    )
    .w(px(width))
    .h(px(height))
}

// ============================================================================
// Launcher tiles
// ============================================================================

const APPS: [(&str, &str); 6] = [
    ("FF", "Firefox"),
    ("TM", "Terminal"),
    ("CD", "Code"),
    ("FI", "Files"),
    ("SP", "Spotify"),
    ("＋", "添加"),
];

fn apps() -> Div {
    h().items_stretch()
        .gap(px(1.))
        .bg(line())
        .border(px(1.))
        .border_color(line())
        .children(APPS.iter().enumerate().map(|(i, (mark, name))| {
            v().id(("app", i))
                .flex_1()
                .min_w_0()
                .items_center()
                .gap(px(12.))
                .py(px(24.))
                .px(px(18.))
                .bg(black())
                .hover(|this| this.bg(surface_2()))
                .child(
                    div()
                        .w(px(42.))
                        .h(px(42.))
                        .flex_shrink_0()
                        .rounded_full()
                        .border(px(1.))
                        .border_color(line_strong())
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(txt(*mark, 13., FontWeight::NORMAL, muted())),
                )
                .child(caps(name, 10.5, FontWeight::NORMAL, muted(), 0.1))
        }))
}

// ============================================================================
// Tracked text without a baked color (inherits from the parent)
// ============================================================================

fn caps_no_color(raw: &str, size: f32, weight: FontWeight, em: f32) -> Div {
    let gap = size * em;
    let mut row = h().items_center().flex_shrink_0();
    for ch in raw.to_uppercase().chars() {
        row = row.child(
            div()
                .mr(px(gap))
                .text_size(px(size))
                .line_height(px(size * 1.538))
                .font_weight(weight)
                .child(ch.to_string()),
        );
    }
    row
}

// ============================================================================

fn main() {
    let bounds = Bounds {
        origin: point(px(120.), px(80.)),
        size: size(px(1440.), px(1000.)),
    };

    gpui_kit::application().run(move |cx| {
        gpui_kit::init(cx);
        gpui_kit::open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                window_decorations: Some(WindowDecorations::Client),
                titlebar: Some(TitlebarOptions {
                    title: Some("GNOUS".into()),
                    ..Default::default()
                }),
                ..Default::default()
            },
            cx,
            |_, cx| cx.new(App::new),
        )
        .expect("failed to open window");
    });
}
