//! Грамматика CSS Masking для inline-`style` CSSOM (BUG-1408).
//!
//! `element.style.<p> = v` проверяет значение по грамматике свойства и хранит каноническую
//! запись (CSSOM §serialize a CSS value); недопустимое значение отбрасывается. Покрыто:
//! `mask-{image,size,repeat,position,origin,clip,composite,mode}`, `clip-path`/`clip`
//! (`clip_cssom`).
//! Слои `mask-{image,size,repeat,position*}` — та же грамматика, что у `background-*`
//! (`background_cssom`), поэтому проверка делегируется ей; `mask-composite`/`mask-mode`/
//! `mask-{origin,clip}` — свои списки ключевых слов. Вызывается из JS-шима
//! (`_lumen_css_canonical_mask`); каскад этим кодом не пользуется (его разборщики мягкие).

use super::background_cssom::{canonical_specified_background, list, one_of};
use super::length::split_top_level_ws;

/// `<geometry-box> = <shape-box> | fill-box | stroke-box | view-box`.
const COORD_BOXES: &[&str] = &[
    "border-box",
    "padding-box",
    "content-box",
    "fill-box",
    "stroke-box",
    "view-box",
];

/// Каноническая запись `value` свойства `prop` либо `None`, если значение не по грамматике
/// (присваивание игнорируется). `None` и для свойства, которого здесь нет.
pub fn canonical_specified_mask(prop: &str, value: &str) -> Option<String> {
    let v = value.trim();
    if v.is_empty() {
        return None;
    }
    match prop {
        "mask-image" => canonical_specified_background("background-image", v),
        "mask-size" => canonical_specified_background("background-size", v),
        "mask-repeat" => canonical_specified_background("background-repeat", v),
        "mask-position" => list(v, mask_position),
        "mask-origin" => list(v, coord_box),
        // CSS Masking L1 §4.5: `<coord-box> | no-clip`.
        "mask-clip" => list(v, |s| one_of(s, COORD_BOXES).or_else(|| one_of(s, &["no-clip"]))),
        // CSS Masking L1 §4.7: `add | subtract | intersect | exclude`.
        "mask-composite" => list(v, |s| one_of(s, &["add", "subtract", "intersect", "exclude"])),
        // CSS Masking L1 §4.3: `alpha | luminance | match-source`.
        "mask-mode" => list(v, |s| one_of(s, &["alpha", "luminance", "match-source"])),
        // CSS Masking L1 §3/§4.5 и CSS 2.1 §11.1 — своя грамматика (`clip_cssom`).
        "clip-path" | "clip" => super::clip_cssom::canonical_specified_clip(prop, v),
        _ => None,
    }
}

fn coord_box(s: &str) -> Option<String> {
    one_of(s, COORD_BOXES)
}

/// `<position>` одного слоя, но трёхзначная запись недопустима: `mask-position-invalid` считает
/// невалидными `right top 5px`, `center left 1px`, `left 4px top`, `bottom right 8%` —
/// все они дают ровно три токена (у `background-position` та же форма принимается).
fn mask_position(v: &str) -> Option<String> {
    if split_top_level_ws(v).len() == 3 {
        return None;
    }
    canonical_specified_background("background-position", v)
}
