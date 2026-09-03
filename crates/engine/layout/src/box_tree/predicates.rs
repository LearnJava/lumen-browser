//! CSS Scrollbars L1 §6.2 scrollbar-gutter reservation and replaced-box
//! element-name predicates (`is_{image,video,canvas,audio,iframe,picture}_element`).
//!
//! Перенесено батчем SPLIT-BT19 из `crates/engine/layout/src/box_tree.rs`
//! (анкер `const SCROLLBAR_GUTTER_AUTO` до конца головного региона файла)
//! без правок тел.

use super::*;

/// Layout-side gutter width for `scrollbar-width: auto` in CSS px.
///
/// Must match `SCROLLBAR_WIDTH` constant in `lumen_paint::display_list` so that
/// the space reserved in layout equals the painted scrollbar track width.
// CSS: scrollbar-width — P4: if the paint-side constant changes, update this too.
const SCROLLBAR_GUTTER_AUTO: f32 = 12.0;

/// Layout-side gutter width for `scrollbar-width: thin` in CSS px.
///
/// Must match `SCROLLBAR_WIDTH_THIN` in `lumen_paint::display_list`.
// CSS: scrollbar-width — P4: keep in sync with SCROLLBAR_WIDTH_THIN in display_list.rs.
const SCROLLBAR_GUTTER_THIN: f32 = 6.0;

/// CSS Scrollbars L1 §6.2 — inline-axis (horizontal) scrollbar gutter reservation.
///
/// Lumen renders overlay scrollbars, so by default (`scrollbar-gutter: auto`)
/// the scrollbar track overlaps content and no space is reserved in layout.
/// With `scrollbar-gutter: stable`, the UA must always reserve the gutter to
/// prevent layout shift when the scrollbar appears or disappears.
///
/// Returns the CSS px width to subtract from `content_width` before laying out children.
/// Only non-zero when `overflow-y` is `scroll`, `auto` or `hidden` AND `scrollbar-gutter`
/// is `stable` or `stable both-edges` AND `scrollbar-width` is not `none`. `hidden`
/// establishes a scroll container per CSS Overflow L3 §3.3 (still programmatically
/// scrollable via script even though the UA never paints a scrollbar for it), so
/// `stable` reserves its gutter the same as `scroll`/`auto` — WPT
/// `css/css-overflow/scrollbar-gutter-001.html` "overflow hidden, scrollbar-gutter
/// stable" asserts exactly this. `visible` and `clip` are excluded: `visible` never
/// establishes a scroll container, and `clip` explicitly disables the scrolling
/// machinery outright (CSS Overflow L3 §3.4), so neither can ever show a scrollbar.
///
// CSS: scrollbar-width, scrollbar-gutter — P4: verify SCROLLBAR_GUTTER_* match
// SCROLLBAR_WIDTH / SCROLLBAR_WIDTH_THIN in lumen_paint::display_list.
pub(crate) fn scrollbar_gutter_inline(s: &ComputedStyle) -> f32 {
    let can_scroll_y = matches!(
        s.overflow_y,
        Overflow::Scroll | Overflow::Auto | Overflow::Hidden
    );
    if !can_scroll_y {
        return 0.0;
    }
    let unit = match s.scrollbar_width {
        ScrollbarWidth::None => return 0.0,
        ScrollbarWidth::Auto => SCROLLBAR_GUTTER_AUTO,
        ScrollbarWidth::Thin => SCROLLBAR_GUTTER_THIN,
    };
    match s.scrollbar_gutter {
        ScrollbarGutter::Auto => 0.0,
        // `stable` reserves gutter on the end edge only.
        ScrollbarGutter::Stable => unit,
        // `stable both-edges` mirrors the gutter on the start edge as well
        // so the content remains centred even when the scrollbar appears.
        ScrollbarGutter::StableBothEdges => unit * 2.0,
    }
}

/// CSS Scrollbars L1 §6.2 — inline-start-edge offset for `scrollbar-gutter:
/// stable both-edges`.
///
/// `scrollbar_gutter_inline` already removes `2 × unit` from `content_width`
/// for `both-edges`, but that alone only narrows the content box — it leaves
/// children flush against the same inline-start edge as `stable`'s
/// end-edge-only reservation. `both-edges` mirrors the gutter onto the start
/// edge too, so children must additionally start `unit` further in. Returns
/// `0.0` for `Stable`/`Auto` (their reservation is end-edge-only, no shift)
/// and for `Overflow::Visible`/`Overflow::Clip` (same eligibility as
/// `scrollbar_gutter_inline`). WPT `css/css-overflow/scrollbar-gutter-001.html`
/// "overflow …, scrollbar-gutter stable both-edges" asserts
/// `container.offsetLeft < content.offsetLeft` for exactly this reason.
///
/// `both-edges` shifts the same physical amount regardless of direction — a
/// gutter is reserved on both physical sides, so the start edge always moves
/// in by one unit no matter which logical edge is "start". Plain `stable`
/// differs: in LTR it reserves the gutter on the physical *right* (inline-end),
/// so the physical-left origin never moves — but in RTL the inline-end is the
/// physical *left*, so the gutter sits where children start from and the
/// whole content box must shift right by the full unit instead. WPT
/// `css/css-overflow/scrollbar-gutter-rtl-001.html` asserts exactly this:
/// `container.offsetLeft < content.offsetLeft` for plain `stable` once
/// `direction: rtl` is in effect, not just for `stable both-edges`.
pub(crate) fn scrollbar_gutter_inline_start(s: &ComputedStyle) -> f32 {
    match s.scrollbar_gutter {
        // Symmetric reservation — direction-independent.
        ScrollbarGutter::StableBothEdges => scrollbar_gutter_inline(s) / 2.0,
        // Single-edge reservation lands on the physical-left origin only
        // under RTL (inline-end == physical left there).
        ScrollbarGutter::Stable if s.direction == Direction::Rtl => scrollbar_gutter_inline(s),
        _ => 0.0,
    }
}

/// CSS Scrollbars L1 §6.2 — block-axis (vertical) scrollbar gutter reservation.
///
/// Returns the CSS px height to subtract from available content height when a
/// horizontal scrollbar's gutter must be reserved (`overflow-x: scroll/auto` +
/// `scrollbar-gutter: stable`). `both-edges` is not defined for the block axis
/// by the spec, so only one gutter unit is reserved regardless.
///
// CSS: scrollbar-width, scrollbar-gutter — the block-axis gutter reduces the
// content height handed to children (see `children_available_height`), mirroring
// the inline-axis `scrollbar_gutter_inline` reduction of `content_width`. Includes
// `hidden` for the same reason `scrollbar_gutter_inline` does — see its doc comment.
pub(crate) fn scrollbar_gutter_block(s: &ComputedStyle) -> f32 {
    let can_scroll_x = matches!(
        s.overflow_x,
        Overflow::Scroll | Overflow::Auto | Overflow::Hidden
    );
    if !can_scroll_x {
        return 0.0;
    }
    let unit = match s.scrollbar_width {
        ScrollbarWidth::None => return 0.0,
        ScrollbarWidth::Auto => SCROLLBAR_GUTTER_AUTO,
        ScrollbarWidth::Thin => SCROLLBAR_GUTTER_THIN,
    };
    match s.scrollbar_gutter {
        ScrollbarGutter::Auto => 0.0,
        ScrollbarGutter::Stable | ScrollbarGutter::StableBothEdges => unit,
    }
}

/// HTML-имя элемента `<img>` для распознавания replaced-боксов в layout.
/// Tag-name в DOM хранится lower-case (HTML5 tree-builder), поэтому
/// сравнение точное, без `eq_ignore_ascii_case`.
pub(crate) fn is_image_element(doc: &Document, id: NodeId) -> bool {
    matches!(
        &doc.get(id).data,
        NodeData::Element { name, .. } if name.local == "img"
    )
}

/// HTML-имя `<video>` для распознавания media replaced-боксов в layout.
pub(crate) fn is_video_element(doc: &Document, id: NodeId) -> bool {
    matches!(
        &doc.get(id).data,
        NodeData::Element { name, .. } if name.local == "video"
    )
}

/// HTML-имя `<canvas>` для распознавания replaced-боксов рисовалки в layout.
pub(crate) fn is_canvas_element(doc: &Document, id: NodeId) -> bool {
    matches!(
        &doc.get(id).data,
        NodeData::Element { name, .. } if name.local == "canvas"
    )
}

/// HTML-имя `<audio>` для распознавания media replaced-боксов в layout.
pub(crate) fn is_audio_element(doc: &Document, id: NodeId) -> bool {
    matches!(
        &doc.get(id).data,
        NodeData::Element { name, .. } if name.local == "audio"
    )
}

/// HTML-имя `<iframe>` для распознавания встроенных документов в layout.
pub(crate) fn is_iframe_element(doc: &Document, id: NodeId) -> bool {
    matches!(
        &doc.get(id).data,
        NodeData::Element { name, .. } if name.local == "iframe"
    )
}

/// HTML-имя `<picture>` — обёртка над `<source>`-кандидатами и одним
/// `<img>`-fallback-ом. Сам по себе пиктур ничего не рендерит, его
/// единственная роль — переадресовать source-selection на inner `<img>`.
pub(crate) fn is_picture_element(doc: &Document, id: NodeId) -> bool {
    matches!(
        &doc.get(id).data,
        NodeData::Element { name, .. } if name.local == "picture"
    )
}
