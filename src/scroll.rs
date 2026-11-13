//! LED-tick scroll indicator (design 10 of `demo/scroll-indicators`).
//!
//! The scroll areas no longer rely on a wheel: the tick column on their right
//! edge *is* the scrubber. Click anywhere on it to jump, hold and drag to scan,
//! or use ↑/↓/PgUp/PgDn/Home/End. The column is 26 discrete ticks — never a
//! continuous thumb — so the indicator stays honest about being coarse while
//! the pointer mapping underneath is pixel-accurate.

use gpui_kit::{
    Bounds, Context, Div, Empty, IntoElement, ParentElement as _, Pixels, Render, ScrollHandle,
    Styled, Window, div, point, px,
};

use crate::theme::{accent, alpha};

/// Discrete steps in the column. Also the visual granularity of the readout.
pub const TICKS: usize = 26;
/// Tick height in px; the gap falls out of `justify_between`.
const TICK_HEIGHT: f32 = 2.;
/// Vertical breathing room at both ends of the column. Shared by the paint and
/// the pointer→progress mapping so the tick under the cursor is the real one.
pub const INSET: f32 = 12.;

/// Drag payload for the tick column. Renders nothing: it exists so GPUI keeps
/// delivering move events after the pointer leaves the 20px-wide column.
#[derive(Clone)]
pub struct DragLed;

impl Render for DragLed {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        Empty
    }
}

/// Scroll state for one area, plus the geometry the tick column needs.
///
/// All values come from the shared [`ScrollHandle`] that the scrollable div
/// tracks, so painting, hit testing and the keyboard can never disagree.
#[derive(Default)]
pub struct Led {
    handle: ScrollHandle,
}

impl Led {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn handle(&self) -> &ScrollHandle {
        &self.handle
    }

    /// Scrollable distance in px (0 when the content fits).
    fn max(&self) -> f32 {
        self.handle.max_offset().y.as_f32().max(0.)
    }

    /// Distance already scrolled, 0 at the top.
    fn offset(&self) -> f32 {
        (-self.handle.offset().y.as_f32()).max(0.)
    }

    pub fn is_scrollable(&self) -> bool {
        self.max() > 0.5
    }

    /// 0 at the top, 1 at the bottom.
    pub fn progress(&self) -> f32 {
        let max = self.max();
        if max <= 0.5 {
            0.
        } else {
            (self.offset() / max).clamp(0., 1.)
        }
    }

    pub fn set_progress(&self, progress: f32) {
        let progress = progress.clamp(0., 1.);
        self.handle
            .set_offset(point(px(0.), px(-progress * self.max())));
    }

    pub fn scroll_by(&self, dy: f32) {
        let target = (self.offset() + dy).clamp(0., self.max());
        self.handle.set_offset(point(px(0.), px(-target)));
    }

    /// One screen less a line or two of overlap, like a browser's PageDown.
    pub fn page_by(&self, direction: f32) {
        let height = self.handle.bounds().size.height.as_f32();
        self.scroll_by(direction * ((height - 40.).max(120.) * 0.9));
    }

    pub fn to_edge(&self, end: bool) {
        self.set_progress(if end { 1. } else { 0. });
    }

    /// Map a window-space `y` inside `bounds` to a scroll progress.
    pub fn progress_at(&self, y: Pixels, bounds: Bounds<Pixels>) -> f32 {
        let track = (bounds.size.height - px(INSET * 2.)).as_f32().max(1.);
        ((y - bounds.top() - px(INSET)).as_f32() / track).clamp(0., 1.)
    }
}

/// The tick column itself: 26 left-anchored bars, filled ones dim red, the
/// current one long and solid. `progress` is 0..=1.
pub fn ticks(progress: f32) -> Div {
    let lit = progress * TICKS as f32;
    let current = if progress >= 0.999 {
        TICKS - 1
    } else {
        (lit.floor() as usize).min(TICKS - 1)
    };

    let mut column = div()
        .flex()
        .flex_col()
        .items_end()
        .justify_between()
        .h_full()
        .py(px(INSET));

    for index in 0..TICKS {
        let (width, color) = if index == current {
            (14., accent())
        } else if (index as f32) < lit {
            (8., alpha(0xe31937_8c))
        } else {
            (5., alpha(0xffffff1f))
        };
        column = column.child(
            div()
                .flex_shrink_0()
                .h(px(TICK_HEIGHT))
                .w(px(width))
                .bg(color),
        );
    }
    column
}
