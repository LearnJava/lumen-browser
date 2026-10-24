//! `zoom` baked into computed lengths after the cascade (CSS Viewport L1 §5).
//!
//! Moved out of `cascade.rs` unchanged (DEVX-22, file-size gate).

use lumen_css_parser::Declaration;
use crate::style::{
    parse_css_wide_keyword, ComputedStyle, FilterFn, FlexBasis, Length, LengthOrAuto, TextDecorationThickness,
    TransformFn,
};

fn zoom_length(len: &mut Length, z: f32, rem_k: f32, root_k: f32) {
    // Viewport units resolve against the unzoomed viewport, so the factor is
    // folded into the coefficient (CSS Viewport L1 §5: `1vh` under `zoom: 2`
    // is twice as tall as outside it). The font-relative units are not listed:
    // their basis (`font_size`, the font metrics) already carries the zoom.
    if let Length::Px(v) | Length::Vh(v) | Length::Vw(v) | Length::Vmin(v) | Length::Vmax(v) = len {
        *v *= z;
    }
    // `rem` resolves against the fixed 16px UA constant, so the root's
    // (zoomed) font-size is folded into the coefficient the same way.
    if let Length::Rem(v) = len {
        *v *= rem_k;
    }
    // `rlh`/`rex`/`rch` carry the root font's (already root-zoomed) metrics, so
    // only the zoom this element adds on top of the root's remains.
    if let Length::Rlh(v) | Length::Rex(v) | Length::Rch(v) = len {
        *v *= root_k;
    }
}

/// Same for a `<length> | auto` field — `auto` carries no length to scale.
fn zoom_length_or_auto(len: &mut LengthOrAuto, z: f32, rem_k: f32, root_k: f32) {
    if let LengthOrAuto::Length(l) = len {
        zoom_length(l, z, rem_k, root_k);
    }
}

/// CSS Viewport L1 §5 — fold the element's effective `zoom` into its computed
/// box-model lengths.
///
/// Runs after the main cascade pass, so it sees the winning declarations. Every
/// property scaled here is **non-inherited**, which is what makes a blanket
/// multiply correct: the value is either specified on this element (and so has
/// not been scaled by anyone) or is the initial `0`/`auto`/`none` (where
/// scaling is a no-op). Inherited length properties are deliberately absent —
/// they arrive already carrying the ancestors' zoom, so touching them would
/// double-apply it.
///
/// `font_size` is handled by the caller rather than here, because it is the one
/// value whose correct factor depends on whether the element specified it (see
/// the call site).
pub(super) fn apply_zoom_to_lengths(style: &mut ComputedStyle, z: f32, rem_k: f32, root_k: f32, declared: ZoomDeclared) {
    if (z - 1.0).abs() < f32::EPSILON && (rem_k - 1.0).abs() < f32::EPSILON && (root_k - 1.0).abs() < f32::EPSILON {
        return;
    }
    for len in [
        &mut style.width,
        &mut style.height,
        &mut style.min_width,
        &mut style.max_width,
        &mut style.min_height,
        &mut style.max_height,
    ] {
        if let Some(l) = len.as_mut() {
            zoom_length(l, z, rem_k, root_k);
        }
    }
    for len in [
        &mut style.margin_top,
        &mut style.margin_right,
        &mut style.margin_bottom,
        &mut style.margin_left,
        &mut style.top,
        &mut style.right,
        &mut style.bottom,
        &mut style.left,
    ] {
        zoom_length_or_auto(len, z, rem_k, root_k);
    }
    for len in [
        &mut style.padding_top,
        &mut style.padding_right,
        &mut style.padding_bottom,
        &mut style.padding_left,
        &mut style.row_gap,
        &mut style.column_gap,
    ] {
        zoom_length(len, z, rem_k, root_k);
    }
    // Border widths are already resolved to px by the cascade.
    style.border_top_width *= z;
    style.border_right_width *= z;
    style.border_bottom_width *= z;
    style.border_left_width *= z;
    scale_visual_lengths(style, z, rem_k, root_k);
    scale_inherited_lengths(style, z, rem_k, root_k, declared);
}

/// Which of the inherited length properties `scale_inherited_lengths` touches were set
/// by a winning declaration on the element itself (as opposed to inherited, `unset`,
/// `initial` or never specified).
#[derive(Clone, Copy, Default)]
pub(crate) struct ZoomDeclared {
    text_shadow: bool,
    border_spacing: bool,
    text_indent: bool,
    letter_spacing: bool,
    word_spacing: bool,
    decoration_thickness: bool,
    underline_offset: bool,
    text_stroke: bool,
}

impl ZoomDeclared {
    /// Every property marked declared: `getComputedStyle()` divides all of them by the
    /// element's `effective_zoom`, inherited or not, because each already carries it.
    pub(crate) const ALL: Self = Self {
        text_shadow: true,
        border_spacing: true,
        text_indent: true,
        letter_spacing: true,
        word_spacing: true,
        decoration_thickness: true,
        underline_offset: true,
        text_stroke: true,
    };

    /// Scans the cascade-sorted declarations; the last one touching a property decides.
    pub(super) fn of<'a>(decls: impl Iterator<Item = &'a Declaration>) -> Self {
        let mut d = Self::default();
        for decl in decls {
            let set = parse_css_wide_keyword(decl.value.trim()).is_none();
            match decl.property.as_str() {
                "text-shadow" => d.text_shadow = set,
                "border-spacing" => d.border_spacing = set,
                "text-indent" => d.text_indent = set,
                "letter-spacing" => d.letter_spacing = set,
                "word-spacing" => d.word_spacing = set,
                "text-decoration" | "text-decoration-thickness" => d.decoration_thickness = set,
                "text-underline-offset" => d.underline_offset = set,
                "-webkit-text-stroke" | "-webkit-text-stroke-width" => d.text_stroke = set,
                // `all` resets (or, for `inherit`, copies from the parent) every one of them.
                "all" => d = Self::default(),
                _ => {}
            }
        }
        d
    }
}

/// The *inherited* lengths `zoom` scales (BUG-1592): `text-shadow`, `border-spacing`,
/// `text-indent`, `letter-spacing`, `word-spacing`, `text-decoration-thickness`,
/// `text-underline-offset`, `-webkit-text-stroke-width`. A value inherited from the
/// parent already carries every ancestor's zoom, so only a property `declared` on this
/// element takes the element's `effective_zoom` (`z`) — the same rule `font-size` follows.
pub(crate) fn scale_inherited_lengths(style: &mut ComputedStyle, z: f32, rem_k: f32, root_k: f32, declared: ZoomDeclared) {
    if declared.text_shadow {
        for shadow in &mut style.text_shadow {
            shadow.offset_x *= z;
            shadow.offset_y *= z;
            shadow.blur *= z;
        }
    }
    if declared.border_spacing {
        style.border_spacing_h *= z;
        style.border_spacing_v *= z;
    }
    if declared.text_indent {
        zoom_length(&mut style.text_indent, z, rem_k, root_k);
    }
    if declared.letter_spacing {
        style.letter_spacing *= z;
    }
    if declared.word_spacing {
        style.word_spacing *= z;
    }
    if declared.decoration_thickness {
        match &mut style.text_decoration_thickness {
            TextDecorationThickness::Length(v) | TextDecorationThickness::LengthPercentage { px: v, .. } => *v *= z,
            _ => {}
        }
    }
    if declared.underline_offset
        && let Some(v) = style.text_underline_offset.as_mut()
    {
        *v *= z;
    }
    if declared.text_stroke {
        style.text_stroke_width *= z;
    }
}

/// The non-inherited paint/transform lengths `zoom` scales on top of the box model:
/// border radii, `box-shadow`, `outline-width`/`-offset`, `flex-basis`,
/// `contain-intrinsic-*`, `scroll-margin`/`-padding`, `perspective`, `translate`
/// and the offsets inside `transform` (BUG-1592). `getComputedStyle()` calls it
/// with `1 / z` to report the unzoomed computed value (CSS Viewport L1 §5).
pub(crate) fn scale_visual_lengths(style: &mut ComputedStyle, z: f32, rem_k: f32, root_k: f32) {
    for len in [
        &mut style.border_top_left_radius,
        &mut style.border_top_right_radius,
        &mut style.border_bottom_right_radius,
        &mut style.border_bottom_left_radius,
        &mut style.border_top_left_radius_y,
        &mut style.border_top_right_radius_y,
        &mut style.border_bottom_right_radius_y,
        &mut style.border_bottom_left_radius_y,
        &mut style.outline_offset,
    ] {
        zoom_length(len, z, rem_k, root_k);
    }
    for shadow in &mut style.box_shadow {
        shadow.offset_x *= z;
        shadow.offset_y *= z;
        shadow.blur *= z;
        shadow.spread *= z;
    }
    style.outline_width *= z;
    for f in &mut style.filter {
        if let FilterFn::Blur(r) = f {
            *r *= z;
        }
    }
    if let FlexBasis::Length(l) = &mut style.flex_basis {
        zoom_length(l, z, rem_k, root_k);
    }
    for len in [&mut style.contain_intrinsic_width, &mut style.contain_intrinsic_height] {
        if let Some(l) = len.as_mut() {
            zoom_length(l, z, rem_k, root_k);
        }
    }
    for v in [
        &mut style.scroll_margin_top,
        &mut style.scroll_margin_right,
        &mut style.scroll_margin_bottom,
        &mut style.scroll_margin_left,
        &mut style.scroll_padding_top,
        &mut style.scroll_padding_right,
        &mut style.scroll_padding_bottom,
        &mut style.scroll_padding_left,
    ] {
        *v *= z;
    }
    if let Some(p) = style.perspective.as_mut() {
        *p *= z;
    }
    if let Some((x, y)) = style.translate.as_mut() {
        zoom_length(x, z, rem_k, root_k);
        zoom_length(y, z, rem_k, root_k);
    }
    for f in &mut style.transform {
        match f {
            TransformFn::Translate(x, y) => {
                *x *= z;
                *y *= z;
            }
            TransformFn::TranslateX(v) | TransformFn::TranslateY(v) | TransformFn::TranslateZ(v) | TransformFn::Perspective(v) => *v *= z,
            TransformFn::Translate3d(x, y, w) => {
                *x *= z;
                *y *= z;
                *w *= z;
            }
            TransformFn::TranslateLp(x, y, w) => {
                zoom_length(x, z, rem_k, root_k);
                zoom_length(y, z, rem_k, root_k);
                *w *= z;
            }
            // `matrix(a b c d e f)` / `matrix3d`: only the translation column carries a length.
            TransformFn::Matrix(m) => {
                m[4] *= z;
                m[5] *= z;
            }
            TransformFn::Matrix3d(m) => {
                m[12] *= z;
                m[13] *= z;
                m[14] *= z;
            }
            _ => {}
        }
    }
}
