mod icons;
mod theme;

use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AppContext as _, Bounds, Context, Div, FontWeight, Hsla, InteractiveElement as _,
    IntoElement, ParentElement as _, PathBuilder, Render, StatefulInteractiveElement as _,
    Styled, TitlebarOptions, Window, WindowBounds, WindowOptions, canvas, div, point, px, relative,
    rgba, size,
};
use theme::{
    FONT, accent, accent_hover, alpha, black, caps, dim, h, line, line_faint, line_strong, muted,
    ok, surface_2, text, tracked, txt, v, warn, white,
};

// ============================================================================
// App
// ============================================================================

struct App;

impl Render for App {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        h()
            .size_full()
            .items_stretch()
            .bg(black())
            .font_family(FONT)
            .text_color(text())
            .child(rail())
            .child(
                v()
                    .flex_1()
                    .min_w_0()
                    .min_h_0()
                    .child(topbar())
                    .child(content()),
            )
    }
}

// ============================================================================
// Left icon rail
// ============================================================================

fn rail() -> Div {
    v()
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
        )
        .child(rail_button("rail-overview", icons::GRID, true))
        .child(rail_button("rail-ext", icons::PUZZLE, false))
        .child(rail_button("rail-appearance", icons::SUN, false))
        .child(rail_button("rail-devices", icons::MONITOR, false))
        .child(rail_button("rail-power", icons::POWER, false))
        .child(div().flex_1())
        .child(rail_button("rail-settings", icons::SETTINGS, false))
}

fn rail_button(id: &'static str, svg: &'static str, active: bool) -> impl IntoElement {
    let color = if active { text() } else { dim() };
    div()
        .relative()
        .w(px(40.))
        .h(px(40.))
        .flex()
        .items_center()
        .justify_center()
        .id(id)
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
        .child(icons::icon(svg, 18., color))
}

// ============================================================================
// Top bar
// ============================================================================

fn topbar() -> Div {
    h()
        .h(px(56.))
        .flex_shrink_0()
        .items_center()
        .gap(px(34.))
        .px(px(40.))
        .border_b(px(1.))
        .border_color(line())
        .child(tracked("GNOUS", 13., FontWeight::MEDIUM, text(), 0.34))
        .child(
            h()
                .h_full()
                .items_stretch()
                .gap(px(30.))
                .child(nav_item("Overview", true))
                .child(nav_item("Extensions", false))
                .child(nav_item("Appearance", false))
                .child(nav_item("Windows", false))
                .child(nav_item("Power", false)),
        )
        .child(div().flex_1())
        .child(
            h()
                .items_center()
                .gap(px(22.))
                .text_size(px(12.))
                .text_color(muted())
                .child(
                    h()
                        .items_center()
                        .gap(px(7.))
                        .child(div().size(px(5.)).rounded_full().bg(ok()))
                        .child(div().child("System nominal")),
                )
                .child(div().child("GNOME 46 · Wayland"))
                .child(div().text_color(text()).child("21:04")),
        )
}

fn nav_item(label: &str, active: bool) -> Div {
    let color = if active { text() } else { muted() };
    div()
        .h_full()
        .flex()
        .items_center()
        .border_b(px(1.))
        .border_color(if active { accent() } else { alpha(0x00000000) })
        .child(caps(label, 12., FontWeight::NORMAL, color, 0.13))
}

// ============================================================================
// Scrollable content
// ============================================================================

fn content() -> impl IntoElement {
    v()
        .id("content")
        .flex_1()
        .min_h_0()
        .overflow_y_scroll()
        .pl(px(40.))
        .pr(px(52.))
        .pt(px(52.))
        .pb(px(64.))
        .child(hero())
        .child(metrics())
        .child(section_head("扩展与开关", "管理全部 14 项 →"))
        .child(panels())
        .child(section_head("资源热点", "打开任务管理器 →"))
        .child(processes())
        .child(section_head("启动器 · 常用", "编辑布局 →"))
        .child(apps())
}

fn hero() -> Div {
    h()
        .items_end()
        .justify_between()
        .mb(px(56.))
        .child(
            v()
                .child(
                    caps(
                        "Session · ThinkPad X1 · Wayland",
                        11.,
                        FontWeight::NORMAL,
                        dim(),
                        0.26,
                    )
                    .mb(px(14.)),
                )
                .child(
                    txt("系统运行正常", 44., FontWeight::EXTRA_LIGHT, text())
                        .line_height(px(46.)),
                )
                .child(
                    txt(
                        "已连续运行 6 天 14 小时 · 上次更新于 3 小时前",
                        13.,
                        FontWeight::NORMAL,
                        muted(),
                    )
                    .mt(px(12.)),
                ),
        )
        .child(
            h()
                .items_center()
                .gap(px(10.))
                .child(button("扫描更新", false))
                .child(button("重启会话", true)),
        )
}

fn button(label: &'static str, solid: bool) -> impl IntoElement {
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
    value: &'static str,
    unit: &'static str,
    fill: f32,
    hot: bool,
    left: &'static str,
    right: &'static str,
}

fn metrics() -> Div {
    h()
        .items_stretch()
        .border_t(px(1.))
        .border_color(line())
        .mb(px(58.))
        .child(metric(
            0,
            Metric {
                label: "CPU 负载",
                value: "24",
                unit: "%",
                fill: 0.24,
                hot: false,
                left: "12 核 · 4.2 GHz",
                right: "48°C",
            },
        ))
        .child(metric(
            1,
            Metric {
                label: "内存",
                value: "11.2",
                unit: "/ 32 GB",
                fill: 0.35,
                hot: false,
                left: "SWAP 0.4 GB",
                right: "35%",
            },
        ))
        .child(metric(
            2,
            Metric {
                label: "存储 · 根分区",
                value: "418",
                unit: "/ 512 GB",
                fill: 0.81,
                hot: true,
                left: "Btrfs · NVMe",
                right: "剩余 94 GB",
            },
        ))
        .child(metric(
            3,
            Metric {
                label: "网络",
                value: "2.4",
                unit: "MB/s",
                fill: 0.42,
                hot: false,
                left: "Wi-Fi 6E",
                right: "↓ 2.4 ↑ 0.3",
            },
        ))
}

fn metric(index: usize, m: Metric) -> Div {
    v()
        .w(relative(0.25))
        .min_w_0()
        .pt(px(30.))
        .pb(px(26.))
        .pr(px(28.))
        .when(index > 0, |this| {
            this.pl(px(28.)).border_l(px(1.)).border_color(line())
        })
        .child(caps(m.label, 10.5, FontWeight::NORMAL, dim(), 0.22).mb(px(22.)))
        .child(
            h()
                .items_end()
                .gap(px(8.))
                .child(
                    txt(m.value, 52., FontWeight::EXTRA_LIGHT, text()).line_height(px(52.)),
                )
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
                .child(
                    div()
                        .h_full()
                        .w(relative(m.fill))
                        .bg(if m.hot { accent() } else { text() }),
                ),
        )
        .child(
            h()
                .justify_between()
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
    h()
        .items_end()
        .justify_between()
        .mb(px(22.))
        .child(caps(title, 12., FontWeight::MEDIUM, muted(), 0.24))
        .child(caps(link, 11., FontWeight::NORMAL, dim(), 0.12))
}

// ============================================================================
// Extensions + quick toggles
// ============================================================================

fn panels() -> Div {
    h()
        .items_stretch()
        .gap(px(1.))
        .bg(line())
        .border(px(1.))
        .border_color(line())
        .mb(px(58.))
        .child(
            v()
                .w(relative(0.574))
                .min_w_0()
                .bg(black())
                .pt(px(30.))
                .pb(px(30.))
                .px(px(28.))
                .child(ext_row(
                    0,
                    icons::LIST,
                    "Dash to Dock",
                    "v79 · 运行中",
                    "Active",
                    ok(),
                ))
                .child(ext_row(
                    1,
                    icons::WINDOW,
                    "Blur my Shell",
                    "v57 · 运行中",
                    "Active",
                    ok(),
                ))
                .child(ext_row(
                    2,
                    icons::PLUS,
                    "Just Perfection",
                    "v26 · 已禁用",
                    "Off",
                    dim(),
                ))
                .child(ext_row(
                    3,
                    icons::PULSE,
                    "Vitals",
                    "v73 · 传感器异常",
                    "Warning",
                    warn(),
                ))
                .child(ext_row(
                    4,
                    icons::CLOCK,
                    "Clipboard Indicator",
                    "v54 · 运行中",
                    "Active",
                    ok(),
                )),
        )
        .child(
            v()
                .flex_1()
                .min_w_0()
                .bg(black())
                .pt(px(30.))
                .pb(px(30.))
                .px(px(28.))
                .child(toggle_row(
                    0,
                    "深色界面",
                    "Prefer dark · 全局",
                    true,
                    false,
                ))
                .child(toggle_row(1, "勿扰模式", "屏蔽全部通知", false, false))
                .child(toggle_row(
                    2,
                    "性能模式",
                    "CPU 调度器 → performance",
                    false,
                    false,
                ))
                .child(toggle_row(
                    3,
                    "夜间色温",
                    "22:00 – 07:00 · 3400K",
                    true,
                    false,
                ))
                .child(toggle_row(
                    4,
                    "自动平铺",
                    "窗口管理 · Pop Shell",
                    true,
                    true,
                )),
        )
}

fn ext_row(
    index: usize,
    svg: &'static str,
    name: &'static str,
    meta: &'static str,
    status: &'static str,
    status_color: Hsla,
) -> Div {
    h()
        .items_center()
        .gap(px(16.))
        .py(px(15.))
        .when(index < 4, |this| {
            this.border_b(px(1.)).border_color(line())
        })
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
                .child(icons::icon(svg, 14., muted())),
        )
        .child(
            v()
                .flex_1()
                .min_w_0()
                .child(txt(name, 13., FontWeight::NORMAL, text()))
                .child(txt(meta, 10.5, FontWeight::NORMAL, dim()).mt(px(3.))),
        )
        .child(caps(status, 9.5, FontWeight::NORMAL, status_color, 0.18))
}

fn toggle_row(index: usize, title: &str, desc: &str, on: bool, last: bool) -> Div {
    h()
        .items_center()
        .justify_between()
        .py(px(16.))
        .when(!last, |this| {
            this.border_b(px(1.)).border_color(line())
        })
        .child(
            v()
                .child(txt(title, 13., FontWeight::NORMAL, text()))
                .child(txt(desc, 11., FontWeight::NORMAL, dim()).mt(px(4.))),
        )
        .child(switch(index, on))
}

fn switch(id: usize, on: bool) -> impl IntoElement {
    div()
        .id(("switch", id))
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
// Process table
// ============================================================================

fn processes() -> Div {
    v()
        .border(px(1.))
        .border_color(line())
        .pt(px(26.))
        .pb(px(26.))
        .px(px(28.))
        .mb(px(58.))
        .child(
            h()
                .items_end()
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
                .child(
                    div()
                        .w(px(162.))
                        .flex()
                        .justify_end()
                        .child(caps("CPU", 10., FontWeight::MEDIUM, dim(), 0.2)),
                )
                .child(
                    div()
                        .w(px(162.))
                        .flex()
                        .justify_end()
                        .child(caps("内存", 10., FontWeight::MEDIUM, dim(), 0.2)),
                )
                .child(
                    div()
                        .w(px(240.))
                        .flex()
                        .justify_end()
                        .child(caps("趋势", 10., FontWeight::MEDIUM, dim(), 0.2)),
                ),
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
        .child(
            div()
                .w(px(162.))
                .flex()
                .justify_end()
                .child(txt(cpu, 12.5, FontWeight::NORMAL, muted())),
        )
        .child(
            div()
                .w(px(162.))
                .flex()
                .justify_end()
                .child(txt(mem, 12.5, FontWeight::NORMAL, muted())),
        )
        .child(
            div()
                .w(px(240.))
                .flex()
                .justify_end()
                .child(sparkline(spark, 64., 16., rgba(0xffffffb3))),
        )
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
        .child(
            div()
                .w(px(162.))
                .flex()
                .justify_end()
                .child(txt(cpu, 12.5, FontWeight::NORMAL, muted())),
        )
        .child(
            div()
                .w(px(162.))
                .flex()
                .justify_end()
                .child(txt(mem, 12.5, FontWeight::NORMAL, muted())),
        )
        .child(
            div()
                .w(px(240.))
                .flex()
                .justify_end()
                .child(div().w(px(64.)).h(px(1.)).bg(rgba(0xffffff59))),
        )
}

fn process_row_base(index: usize) -> Div {
    h()
        .items_center()
        .py(px(14.))
        .when(index < 3, |this| {
            this.border_b(px(1.)).border_color(line_faint())
        })
}

fn name_cell(name: &'static str, pid: &'static str) -> Div {
    h()
        .flex_1()
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
                let p = point(
                    bounds.origin.x + px(x * sx),
                    bounds.origin.y + px(y * sy),
                );
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
    h()
        .items_stretch()
        .gap(px(1.))
        .bg(line())
        .border(px(1.))
        .border_color(line())
        .children(APPS.iter().enumerate().map(|(i, (mark, name))| {
            v()
                .id(("app", i))
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
                titlebar: Some(TitlebarOptions {
                    title: Some("GNOUS".into()),
                    ..Default::default()
                }),
                ..Default::default()
            },
            cx,
            |_, cx| cx.new(|_| App),
        )
        .expect("failed to open window");
    });
}
