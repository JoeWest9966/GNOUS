//! Inline line icons, copied verbatim from the demo so stroke weight and
//! geometry match exactly (Lucide's bundled icons use a heavier stroke).

use gpui_kit::component::Icon;
use gpui_kit::{Hsla, Styled, px};

macro_rules! icon {
    ($body:expr) => {
        concat!(
            r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.3" stroke-linecap="round" stroke-linejoin="round">"#,
            $body,
            "</svg>"
        )
    };
}

pub const GRID: &str = icon!(
    r#"<rect x="3" y="3" width="7" height="7"/><rect x="14" y="3" width="7" height="7"/><rect x="3" y="14" width="7" height="7"/><rect x="14" y="14" width="7" height="7"/>"#
);
pub const PUZZLE: &str = icon!(r#"<path d="M10 3h4v4h4v4h4v4h-4v4h-4v-4H10v4H6v-4H2v-4h4V7h4z"/>"#);
pub const SUN: &str = icon!(
    r#"<circle cx="12" cy="12" r="4"/><path d="M12 2v3M12 19v3M2 12h3M19 12h3M5 5l2 2M17 17l2 2M19 5l-2 2M7 17l-2 2"/>"#
);
pub const MONITOR: &str =
    icon!(r#"<rect x="2" y="4" width="20" height="13"/><path d="M8 21h8M12 17v4"/>"#);
pub const POWER: &str = icon!(r#"<path d="M12 2v10"/><path d="M6 6a8 8 0 1 0 12 0"/>"#);
pub const STARTUP: &str = icon!(r#"<path d="M12 17V4M6 10l6-6 6 6"/><path d="M4 20h16"/>"#);
/// Sandbox box: a cube with a visible seam, so it reads as a container and
/// not as another generic square in the rail.
pub const BOX: &str = icon!(
    r#"<path d="M12 3l8 4.5v9L12 21l-8-4.5v-9L12 3z"/><path d="M4 7.5l8 4.5 8-4.5"/><path d="M12 12v9"/>"#
);
pub const SETTINGS: &str = icon!(
    r#"<circle cx="12" cy="12" r="3"/><path d="M19 12a7 7 0 0 0-.1-1l2-1.6-2-3.4-2.4 1a7 7 0 0 0-1.7-1L14.5 3h-4l-.3 2.6a7 7 0 0 0-1.7 1l-2.4-1-2 3.4 2 1.6a7 7 0 0 0 0 2l-2 1.6 2 3.4 2.4-1a7 7 0 0 0 1.7 1l.3 2.6h4l.3-2.6a7 7 0 0 0 1.7-1l2.4 1 2-3.4-2-1.6c.1-.3.1-.7.1-1z"/>"#
);

/// Render an inline icon at an explicit size.
pub fn icon(svg: &'static str, size: f32, color: Hsla) -> impl gpui_kit::IntoElement {
    Icon::default()
        .data(svg.as_bytes())
        .text_color(color)
        .w(px(size))
        .h(px(size))
}
