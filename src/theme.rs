//! Design tokens and small styling helpers for the GNOUS shell.
//!
//! Values mirror `demo/ui-directions/a-cockpit.html` (A · Control Deck).

use gpui_kit::{Div, FontWeight, Hsla, ParentElement as _, Styled, div, px, rgb, rgba};

/// Primary UI family. The demo resolves to Noto Sans CJK on this machine.
pub const FONT: &str = "Noto Sans CJK SC";

/// Line height factor the demo's browser used for `line-height: normal` with
/// this font. GPUI's default is 1.618 (phi), which makes every row taller.
pub const LINE: f32 = 1.538;

// ----- colors -------------------------------------------------------------
pub fn line() -> Hsla {
    rgba(0xffffff17).into()
}
pub fn line_strong() -> Hsla {
    rgba(0xffffff2e).into()
}
pub fn line_faint() -> Hsla {
    rgba(0xffffff0d).into()
}
pub fn text() -> Hsla {
    rgb(0xF5F5F7).into()
}
pub fn muted() -> Hsla {
    rgb(0x86868B).into()
}
pub fn dim() -> Hsla {
    rgb(0x5A5A60).into()
}
pub fn accent() -> Hsla {
    rgb(0xE31937).into()
}
pub fn accent_hover() -> Hsla {
    rgb(0xC2142D).into()
}
pub fn ok() -> Hsla {
    rgb(0x37D67A).into()
}
pub fn warn() -> Hsla {
    rgb(0xF5A623).into()
}
pub fn surface_2() -> Hsla {
    rgb(0x111113).into()
}
/// Alpha color from `0xRRGGBBAA`.
pub fn alpha(hex: u32) -> Hsla {
    rgba(hex).into()
}

pub fn white() -> Hsla {
    rgb(0xFFFFFF).into()
}
pub fn black() -> Hsla {
    rgb(0x000000).into()
}

// ----- layout helpers -----------------------------------------------------
pub fn h() -> Div {
    div().flex().flex_row()
}

pub fn v() -> Div {
    div().flex().flex_col()
}

/// Uppercase label with CSS `letter-spacing` emulated by spacing per glyph
/// (GPUI has no letter-spacing in its text style).
pub fn tracked(raw: &str, size: f32, weight: FontWeight, color: Hsla, em: f32) -> Div {
    let gap = size * em;
    let mut row = h()
        .items_center()
        .flex_shrink_0()
        .text_size(px(size))
        .line_height(px(size * LINE))
        .font_weight(weight)
        .text_color(color);
    for ch in raw.chars() {
        row = row.child(div().mr(px(gap)).child(ch.to_string()));
    }
    row
}

/// Same as [`tracked`] but text is upper-cased first, like the demo's
/// `text-transform: uppercase` labels.
pub fn caps(raw: &str, size: f32, weight: FontWeight, color: Hsla, em: f32) -> Div {
    tracked(&raw.to_uppercase(), size, weight, color, em)
}

pub fn txt(raw: impl Into<String>, size: f32, weight: FontWeight, color: Hsla) -> Div {
    div()
        .flex_shrink_0()
        .text_size(px(size))
        .line_height(px(size * LINE))
        .font_weight(weight)
        .text_color(color)
        .child(raw.into())
}
