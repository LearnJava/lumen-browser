//! `contain` / `contain-intrinsic-size` serialisation for the computed-style map.

use super::length_to_css;
use crate::style::{ContainFlags, Length};

/// Serialises [`ContainFlags`] the way CSS Containment L3 §3 asks for: the
/// `none` keyword when empty, and otherwise the individual keywords in
/// canonical order — never the `strict`/`content` shorthand keywords, which the
/// spec defines as computing to the set they expand to.
pub(super) fn contain_to_css(flags: ContainFlags) -> String {
    if flags == ContainFlags::NONE {
        return "none".into();
    }
    // CSS Containment L3 §3: the full sets serialize as their shorthand keywords.
    if flags == ContainFlags::STRICT {
        return "strict".into();
    }
    if flags == ContainFlags::CONTENT {
        return "content".into();
    }
    let mut out: Vec<&str> = Vec::with_capacity(5);
    for (bit, name) in [
        (ContainFlags::SIZE, "size"),
        (ContainFlags::INLINE_SIZE, "inline-size"),
        (ContainFlags::LAYOUT, "layout"),
        (ContainFlags::STYLE, "style"),
        (ContainFlags::PAINT, "paint"),
    ] {
        if flags.0 & bit.0 != 0 {
            out.push(name);
        }
    }
    out.join(" ")
}

/// Serialises one axis of `contain-intrinsic-size`: `auto? [ none | <length> ]`.
/// The `auto` keyword is behaviourally ignored by layout but is part of the
/// computed value, so it must survive the round-trip (BUG-852).
pub(super) fn contain_intrinsic_to_css(auto: bool, len: Option<&Length>) -> String {
    let base = len.map_or_else(|| "none".to_string(), length_to_css);
    if auto { format!("auto {base}") } else { base }
}
