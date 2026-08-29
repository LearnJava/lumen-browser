//! Пост-каскадные правки `ComputedStyle`: легаси-псевдоэлементы WebKit
//! scrollbar, системные цвета, Forced Colors Mode, рассогласованные оси
//! overflow.
//!
//! Перенесено батчем SPLIT-ST13 из `crates/engine/layout/src/style.rs`
//! (анкер `fn element_can_have_scrollbar`) без правок тел.

#[cfg(test)]
use crate::style::SCROLLBAR_PSEUDO_CASCADES;
use crate::style::{
    compute_pseudo_element_style, with_cascade_index, BackgroundImage, ComputedStyle, CssColor,
    FontVariantEmoji, ForcedColorAdjust, OutlineColor, Overflow, ScrollbarWidth, SvgPaint,
    SystemColor,
};
use lumen_core::geom::Size;
use lumen_css_parser::Stylesheet;
use lumen_dom::{Document, NodeData, NodeId};

/// Whether a scrollbar can ever be shown for `node`, i.e. whether translating
/// `::-webkit-scrollbar*` onto its `scrollbar-width`/`scrollbar-color` can have
/// any effect (BUG-341 S11).
///
/// The condition mirrors paint's own: `lumen_paint::display_list` emits a
/// scrollbar only for a box whose `overflow-x`/`overflow-y` is `scroll` or
/// `auto` (`overflow: hidden` scrolls programmatically but draws no bar), and
/// `box_tree::scrollbar_gutter_{inline,block}` reserve gutter under the same
/// condition. The root element and `<body>` are included regardless: they are
/// the conventional target for styling the *page* scrollbar, and it costs two
/// elements per document to keep that idiom working if the viewport scrollbar
/// ever starts reading its style from them.
///
/// **This is a deliberate behaviour change** (user decision, 2026-07-27),
/// not a pure optimization. Before it, the translation ran on *every* element,
/// so `::-webkit-scrollbar` rules matching a non-scrollable element wrote
/// `scrollbar-width`/`scrollbar-color` there and — both being inherited
/// properties — leaked down to scrollable descendants that matched no rule of
/// their own. WebKit has no such inheritance: `::-webkit-scrollbar` styles the
/// scrollbar of the element it matches. Lumen's leak was an artifact of
/// translating a pseudo-element onto standard inherited properties, so
/// narrowing is also a fidelity fix. The standard `scrollbar-width` /
/// `scrollbar-color` properties are untouched and keep inheriting normally.
fn element_can_have_scrollbar(doc: &Document, node: NodeId, style: &ComputedStyle) -> bool {
    if matches!(style.overflow_x, Overflow::Scroll | Overflow::Auto)
        || matches!(style.overflow_y, Overflow::Scroll | Overflow::Auto)
    {
        return true;
    }
    doc.get(node)
        .element_name()
        .is_some_and(|q| matches!(q.local.as_ref(), "html" | "body"))
}

/// CC-CSS-1: legacy WebKit scrollbar pseudo-elements (`::-webkit-scrollbar`,
/// `::-webkit-scrollbar-thumb`, `::-webkit-scrollbar-track`) are not part of the
/// standard cascade — `PseudoElementKind::Unknown` already parses and matches them
/// (see `pseudo_element_matches`), so this translates their declarations onto the
/// standard `scrollbar-width`/`scrollbar-color` fields, letting pages/chrome that
/// only style scrollbars through the WebKit-only idiom still get a styled result.
/// `-webkit-font-smoothing` needs no handling here: it falls through the ordinary
/// `apply_declaration` catch-all (parsed, then silently ignored) like any other
/// unrecognized property.
///
/// **Runs only for elements that can actually have a scrollbar** — see
/// [`element_can_have_scrollbar`] and BUG-341 "S11" for the behaviour change
/// this implies.
pub(in crate::style) fn apply_webkit_scrollbar_pseudos(
    doc: &Document,
    node: NodeId,
    sheet: &Stylesheet,
    style: &mut ComputedStyle,
    viewport: Size,
    dark_mode: bool,
) {
    // BUG-341 S10: three full pseudo-element cascades per element — 55% of
    // `compute_style` on Lumen's own chrome, and on every other sheet not one of
    // them can match. Node-independent, so it is decided once per sheet.
    if !with_cascade_index(sheet, viewport, dark_mode, |idx| idx.has_webkit_scrollbar_rules) {
        return;
    }
    // BUG-341 S11: and of the sheets that do declare them, only scroll
    // containers can show the result.
    if !element_can_have_scrollbar(doc, node, style) {
        return;
    }
    #[cfg(test)]
    SCROLLBAR_PSEUDO_CASCADES.with(|c| c.set(c.get() + 1));
    let _prof = lumen_core::profile::scope_detail("cs_scrollbar_pseudos");
    // `scrollbar-width` (CSS Scrollbars 1 §2) has no numeric keyword — bucket the
    // pixel width into the closest of the two sized keywords. 9px is the midpoint
    // between `thin`'s 6px and `auto`'s 12px used-width (`display_list.rs`), and
    // matches Lumen's own chrome reference (`docs/design/lumen-v3_3.html`).
    if let Some(bar) =
        compute_pseudo_element_style(doc, node, "-webkit-scrollbar", sheet, style, viewport, dark_mode)
        && let Some(w) = bar.width.as_ref().and_then(|l| l.resolve(bar.font_size, None, viewport))
    {
        style.scrollbar_width = if w <= 0.0 {
            ScrollbarWidth::None
        } else if w <= 9.0 {
            ScrollbarWidth::Thin
        } else {
            ScrollbarWidth::Auto
        };
    }
    let webkit_thumb = compute_pseudo_element_style(
        doc, node, "-webkit-scrollbar-thumb", sheet, style, viewport, dark_mode,
    )
    .and_then(|s| s.background_color)
    .map(|c| c.resolve(style.color));
    let webkit_track = compute_pseudo_element_style(
        doc, node, "-webkit-scrollbar-track", sheet, style, viewport, dark_mode,
    )
    .and_then(|s| s.background_color)
    .map(|c| c.resolve(style.color));
    // Both sides required: `scrollbar-color`'s used value is a (thumb, track) pair,
    // and there is no honest per-side "unset" fallback to reach for here — the UA
    // defaults live one layer down, in `paint::display_list`.
    if let (Some(thumb), Some(track)) = (webkit_thumb, webkit_track) {
        style.scrollbar_color = Some((thumb, track));
    }
}

/// CSS Color 4 §6.2 — resolve `CssColor::System` variants in all CssColor-typed
/// fields of `style` to `CssColor::Rgba` using the element's final used color
/// scheme. Called once at the end of `compute_style`, after all declarations
/// have been applied so `style.color_scheme` is final.
pub(in crate::style) fn resolve_system_colors_in_style(style: &mut ComputedStyle, dark_mode: bool) {
    let dark = style.color_scheme.used_dark(dark_mode);

    macro_rules! resolve_opt {
        ($field:expr) => {
            if let Some(CssColor::System(sc)) = $field {
                *$field = Some(CssColor::Rgba(sc.resolve_color(dark)));
            }
        };
    }
    macro_rules! resolve {
        ($field:expr) => {
            if let CssColor::System(sc) = $field {
                *$field = CssColor::Rgba(sc.resolve_color(dark));
            }
        };
    }

    resolve_opt!(&mut style.background_color);
    resolve!(&mut style.text_decoration_color);
    resolve!(&mut style.text_emphasis_color);
    resolve!(&mut style.border_top_color);
    resolve!(&mut style.border_right_color);
    resolve!(&mut style.border_bottom_color);
    resolve!(&mut style.border_left_color);
    resolve!(&mut style.column_rule_color);
    resolve!(&mut style.gap_rule_color);
}

/// CSS Color Adjustment L1 §3.1 — forces the element's colors to the system
/// palette when Forced Colors Mode is active.
///
/// `forced-color-adjust` is honored: `none` leaves the element untouched;
/// `preserve-parent-color` forces everything except `color`, which keeps its
/// computed (typically inherited, already-forced) value.
///
/// Forced values follow element semantics (§3.1 + HTML UA guidance):
/// links (`a[href]`/`area[href]`) → `LinkText`, disabled controls → `GrayText`,
/// buttons → `ButtonText`/`ButtonFace`/`ButtonBorder`, text fields →
/// `CanvasText`/`Field`; everything else → `CanvasText`/`Canvas`.
/// `box-shadow`/`text-shadow` are forced to none; non-`url()` background
/// images (gradients, cross-fades, `paint()`) are dropped — `url()` images
/// are kept per spec. `background-color` keeps the author's full transparency:
/// an unset or `transparent` background stays transparent.
pub(in crate::style) fn apply_forced_colors_mode(
    doc: &Document,
    node: NodeId,
    style: &mut ComputedStyle,
    dark_mode: bool,
) {
    if style.forced_color_adjust == ForcedColorAdjust::None {
        return;
    }
    let dark = style.color_scheme.used_dark(dark_mode);

    // Element semantics for system-color pair selection.
    let mut is_link = false;
    let mut is_button = false;
    let mut is_field = false;
    let mut is_disabled = false;
    if let NodeData::Element { name, .. } = &doc.get(node).data {
        let tag = name.local.as_str();
        is_link = matches!(tag, "a" | "area") && doc.get(node).get_attr("href").is_some();
        let input_type = if tag == "input" {
            doc.get(node)
                .get_attr("type")
                .map(|s| s.trim().to_ascii_lowercase())
                .unwrap_or_else(|| "text".to_string())
        } else {
            String::new()
        };
        is_button = tag == "button" || matches!(input_type.as_str(), "button" | "submit" | "reset");
        is_field = matches!(tag, "textarea" | "select") || (tag == "input" && !is_button);
        is_disabled = matches!(tag, "input" | "textarea" | "select" | "button")
            && doc.get(node).get_attr("disabled").is_some();
    }

    let fg_kw = if is_disabled {
        SystemColor::GrayText
    } else if is_link {
        SystemColor::LinkText
    } else if is_button {
        SystemColor::ButtonText
    } else {
        SystemColor::CanvasText
    };
    let fg = fg_kw.resolve_color(dark);
    let border = if is_button { SystemColor::ButtonBorder } else { SystemColor::CanvasText }
        .resolve_color(dark);
    let bg = if is_button {
        SystemColor::ButtonFace
    } else if is_field {
        SystemColor::Field
    } else {
        SystemColor::Canvas
    }
    .resolve_color(dark);

    // §3.2 `preserve-parent-color`: only the `color` property escapes forcing.
    if style.forced_color_adjust != ForcedColorAdjust::PreserveParentColor {
        style.color = fg;
    }

    // background-color: forced to the backdrop system color, but the author's
    // full transparency is preserved (unset / alpha 0 stays transparent).
    let bg_visible = match &style.background_color {
        Some(CssColor::Rgba(c)) => c.a > 0,
        Some(CssColor::Wide(w)) => w.a > 0.0,
        // System already resolved to Rgba by resolve_system_colors_in_style;
        // CurrentColor follows the (forced, opaque) `color`.
        Some(CssColor::CurrentColor) | Some(CssColor::System(_)) => true,
        None => false,
    };
    if bg_visible {
        style.background_color = Some(CssColor::Rgba(bg));
    }

    style.border_top_color = CssColor::Rgba(border);
    style.border_right_color = CssColor::Rgba(border);
    style.border_bottom_color = CssColor::Rgba(border);
    style.border_left_color = CssColor::Rgba(border);
    style.column_rule_color = CssColor::Rgba(border);
    style.gap_rule_color = CssColor::Rgba(border);
    if !matches!(style.outline_color, OutlineColor::Auto) {
        style.outline_color = OutlineColor::Color(fg);
    }
    style.text_decoration_color = CssColor::Rgba(fg);
    style.text_emphasis_color = CssColor::Rgba(fg);
    if style.caret_color.is_some() {
        // `auto` (None) already follows the forced `color`.
        style.caret_color = Some(fg);
    }

    // SVG geometry is painted from `fill`/`stroke` (§3.1 lists both).
    if !matches!(style.svg_fill, SvgPaint::None) {
        style.svg_fill = SvgPaint::Color(fg);
    }
    if !matches!(style.svg_stroke, SvgPaint::None) {
        style.svg_stroke = SvgPaint::Color(fg);
    }

    // Shadows are forced to `none`.
    style.box_shadow.clear();
    style.text_shadow.clear();

    // `scrollbar-color` computes to `auto` (§3.1): the system palette owns the
    // scrollbar, an author thumb/track pair would punch a hole in it. `None`
    // *is* the `auto` representation of the field (BUG-388).
    style.scrollbar_color = None;

    // `font-variant-emoji`: «If font-variant-emoji computes to normal or
    // unicode, UAs should force any emoji on the page to its monochrome
    // variant … by forcing the computed value … to text» (§3.1). An explicit
    // `emoji` is the author asking for colour on purpose and survives, as does
    // `text` (already monochrome) — so only the two neutral values move.
    if matches!(style.font_variant_emoji, FontVariantEmoji::Normal | FontVariantEmoji::Unicode) {
        style.font_variant_emoji = FontVariantEmoji::Text;
    }

    // background-image: gradients / cross-fades / paint() are dropped;
    // `url()` images are kept (spec: forced to none unless a url()).
    for layer in &mut style.background_layers {
        if !matches!(layer.image, BackgroundImage::None | BackgroundImage::Url(_)) {
            layer.image = BackgroundImage::None;
        }
    }
}

/// CSS Overflow L3 §2.1: coerce mismatched overflow axes.
/// If one axis is `visible` and the other is not, `visible` becomes `auto`.
pub(in crate::style) fn coerce_overflow_axes(ox: Overflow, oy: Overflow) -> (Overflow, Overflow) {
    let new_ox = if ox == Overflow::Visible && oy != Overflow::Visible { Overflow::Auto } else { ox };
    let new_oy = if oy == Overflow::Visible && ox != Overflow::Visible { Overflow::Auto } else { oy };
    (new_ox, new_oy)
}
