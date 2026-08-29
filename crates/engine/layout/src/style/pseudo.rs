//! Стиль псевдоэлементов: матчинг селекторов на псевдоэлемент, построение
//! стартового (унаследованного) стиля, каскад `::before`/`::after`/
//! `::first-line`/`::first-letter`/`::marker`/`::selection`/UA-псевдоэлементов
//! и слияние с наследованным стилем сегмента (`::first-line`/`::first-letter`).
//!
//! Перенесено батчем SPLIT-ST13 из `crates/engine/layout/src/style.rs`
//! (анкер `fn pseudo_element_name`) без правок тел.

// `PSEUDO_BASE_BUILDS` — тест-инструментовка BUG-341 S10, объявлена в доноре под
// `#[cfg(test)]`; импорт обязан повторять cfg оригинала, иначе обычная сборка
// ловит неразрешённое имя (урок SH-3a о cfg у реэкспорта).
#[cfg(test)]
use crate::style::PSEUDO_BASE_BUILDS;
use crate::style::{
    apply_declaration, apply_font_size, ensure_cascade_index, expand_attr_val, matches_complex,
    note_pseudo_cascade, sheet_targets_pseudo, with_front_cascade_index, ComputedStyle, Content,
    Display, PSEUDO_STATS_ON,
};
use lumen_core::geom::Size;
use lumen_css_parser::{
    ComplexSelector, CompoundSelector, Declaration, PseudoElementKind, SimpleSelector, Specificity,
    Stylesheet,
};
use lumen_dom::{Document, DocumentMode, NodeData, NodeId};

/// Проверяет, является ли `complex` правилом для псевдоэлемента `pseudo`
/// (например "before" для `::before`) на элементе `node`.
/// Если да — возвращает specificity исходного (полного) селектора.
/// Алгоритм: последний compound должен содержать `PseudoElement(pseudo)`;
/// остаток селектора (после удаления этой части) проверяется через
/// существующий `matches_complex`.
/// The name `kind` is written with, without the leading `::`.
///
/// BUG-341 S23: the single source of truth for the kind↔name correspondence.
/// [`pseudo_element_matches`] and [`CascadeIndex::pseudo_subjects`] both go
/// through it, so the sheet-level "does this pseudo appear at all" predicate
/// cannot drift from the matcher it is meant to short-circuit — a drift that
/// would silently drop a pseudo-element's styling, not slow a frame down.
/// Parameterized kinds report the bare name they are spelled with: the
/// argument (`::slotted(sel)`, `::highlight(name)`, `::picker(sel)`) is checked
/// by the matcher, not by the name.
pub(in crate::style) fn pseudo_element_name(kind: &PseudoElementKind) -> &str {
    match kind {
        PseudoElementKind::Before => "before",
        PseudoElementKind::After => "after",
        PseudoElementKind::FirstLine => "first-line",
        PseudoElementKind::FirstLetter => "first-letter",
        PseudoElementKind::Slotted(_) => "slotted",
        PseudoElementKind::Marker => "marker",
        PseudoElementKind::Selection => "selection",
        PseudoElementKind::Placeholder => "placeholder",
        PseudoElementKind::Highlight(_) => "highlight",
        PseudoElementKind::Picker(_) => "picker",
        PseudoElementKind::Checkmark => "checkmark",
        PseudoElementKind::PickerIcon => "picker-icon",
        PseudoElementKind::Unknown(s) => s.as_str(),
    }
}

/// Helper: check if a pseudo-element name matches a PseudoElementKind.
fn pseudo_element_matches(kind: &PseudoElementKind, name: &str) -> bool {
    pseudo_element_name(kind).eq_ignore_ascii_case(name)
}

#[allow(clippy::unwrap_used)]  // унаследовано, docs/lint-policy.md §10
fn matches_complex_for_pseudo(
    complex: &ComplexSelector,
    pseudo: &str,
    doc: &Document,
    node: NodeId,
) -> Option<Specificity> {
    let last = complex.tail.last().map(|(_, c)| c).unwrap_or(&complex.head);
    if !last.parts.iter().any(|p| {
        matches!(p, SimpleSelector::PseudoElement(n) if pseudo_element_matches(n, pseudo))
    }) {
        return None;
    }
    // Строим модифицированный последний compound без PseudoElement.
    let stripped = CompoundSelector {
        parts: last.parts.iter()
            .filter(|p| !matches!(p, SimpleSelector::PseudoElement(_)))
            .cloned()
            .collect(),
    };
    // Собираем модифицированный ComplexSelector.
    let modified = if complex.tail.is_empty() {
        ComplexSelector { head: stripped, tail: vec![] }
    } else {
        let mut tail = complex.tail.clone();
        tail.last_mut().unwrap().1 = stripped;
        ComplexSelector { head: complex.head.clone(), tail }
    };
    if matches_complex(&modified, doc, node) {
        Some(complex.specificity())
    } else {
        None
    }
}

/// CSS Pseudo-Elements L4 §5.5 — true when property `prop` is one of the limited
/// set that applies to the `::marker` pseudo-element: all font properties, the
/// `white-space` property, `color`, the `direction` / `unicode-bidi` /
/// `text-combine-upright` writing-mode properties, `content`, and all animation
/// and transition properties. Custom properties (`--*`) are kept so `var()` inside
/// `content` still resolves. Any other declaration on `::marker` is ignored.
fn marker_property_applies(prop: &str) -> bool {
    let p = prop.trim().to_ascii_lowercase();
    // Custom properties stay available for `var()` substitution inside `content`.
    if p.starts_with("--") {
        return true;
    }
    // `font`, `font-*`, `animation*` and `transition*` families are allowed wholesale.
    p.starts_with("font")
        || p.starts_with("animation")
        || p.starts_with("transition")
        || matches!(
            p.as_str(),
            "color"
                | "content"
                | "white-space"
                | "white-space-collapse"
                | "direction"
                | "unicode-bidi"
                | "text-combine-upright"
        )
}

/// Builds the starting `ComputedStyle` for a pseudo-element of `parent`: every
/// field at its initial value (CSS Pseudo-elements L4 §4 makes `display`
/// `inline`), then every inherited property copied down from the originating
/// element.
///
/// BUG-341 S10: extracted from [`compute_pseudo_element_style`] so it can run
/// *after* the cascade match rather than before it. It costs a 302-field
/// literal plus ~50 field clones, and the overwhelmingly common outcome — no
/// rule matches this element for this pseudo-element — threw all of it away.
/// The profile that found it: `precompute_counters` probes `::before`/`::after`
/// on *every* node to keep `quotes` nesting continuous (1656 calls per chrome
/// layout pass), and `apply_webkit_scrollbar_pseudos` adds three more per
/// element.
fn pseudo_inherited_style(parent: &ComputedStyle) -> ComputedStyle {
    #[cfg(test)]
    PSEUDO_BASE_BUILDS.with(|c| c.set(c.get() + 1));
    // Pseudo-elements inherit from their originating element.
    // Start from root() (all fields at initial values) then override inherited properties.
    // CSS Pseudo-elements L4 §4: default display = inline.
    let mut style = ComputedStyle::root();
    style.display = Display::Inline;
    style.content = Content::Normal;
    // Inherited properties — copy from parent.
    style.color = parent.color;
    style.color_space = parent.color_space;
    style.text_align = parent.text_align;
    style.direction = parent.direction;
    style.font_size = parent.font_size;
    style.line_height = parent.line_height;
    style.line_height_is_relative = parent.line_height_is_relative;
    style.line_height_step = parent.line_height_step;
    style.font_style = parent.font_style;
    style.font_weight = parent.font_weight;
    style.font_variant_caps = parent.font_variant_caps;
    style.font_variant_emoji = parent.font_variant_emoji;
    style.font_stretch = parent.font_stretch;
    style.font_family = parent.font_family.clone();
    style.font_variation_settings = parent.font_variation_settings.clone();
    style.font_feature_settings = parent.font_feature_settings.clone();
    style.font_palette = parent.font_palette.clone();
    style.font_palette_resolved = parent.font_palette_resolved.clone();
    style.text_transform = parent.text_transform;
    style.white_space = parent.white_space;
    style.white_space_collapse = parent.white_space_collapse;
    style.text_indent = parent.text_indent.clone();
    style.letter_spacing = parent.letter_spacing;
    style.word_spacing = parent.word_spacing;
    style.text_decoration_line = parent.text_decoration_line;
    style.text_decoration_color = parent.text_decoration_color;
    style.text_decoration_style = parent.text_decoration_style;
    style.text_decoration_thickness = parent.text_decoration_thickness;
    style.text_emphasis_style = parent.text_emphasis_style.clone();
    style.text_emphasis_color = parent.text_emphasis_color;
    style.text_emphasis_position = parent.text_emphasis_position;
    style.text_underline_position = parent.text_underline_position;
    style.text_underline_offset = parent.text_underline_offset;
    style.text_decoration_skip_ink = parent.text_decoration_skip_ink;
    style.accent_color = parent.accent_color;
    style.color_scheme = parent.color_scheme;
    style.custom_props = parent.custom_props.clone();
    style.visibility = parent.visibility;
    style.cursor = parent.cursor;
    style.text_shadow = parent.text_shadow.clone();
    style.user_select = parent.user_select;
    style.scroll_behavior = parent.scroll_behavior;
    style.tab_size = parent.tab_size;
    style.caret_color = parent.caret_color;
    style.overflow_wrap = parent.overflow_wrap;
    style.word_break = parent.word_break;
    style.line_break = parent.line_break;
    style.hyphens = parent.hyphens;
    style.list_style_type = parent.list_style_type.clone();
    style.list_style_position = parent.list_style_position;
    style.list_style_image = parent.list_style_image.clone();
    style.orphans = parent.orphans;
    style.widows = parent.widows;
    style.scrollbar_width = parent.scrollbar_width;
    style.scrollbar_color = parent.scrollbar_color;
    style.image_rendering = parent.image_rendering;
    style.writing_mode = parent.writing_mode;
    style.text_orientation = parent.text_orientation;
    style.ruby_position = parent.ruby_position;
    style.ruby_align = parent.ruby_align;
    style.ruby_merge = parent.ruby_merge;
    style.math_style = parent.math_style;
    style.math_depth = parent.math_depth;
    style.font_size_adjust = parent.font_size_adjust;
    style.text_wrap_mode = parent.text_wrap_mode;
    style.text_wrap_style = parent.text_wrap_style;
    style.interpolate_size = parent.interpolate_size;
    style.quotes = parent.quotes.clone();
    style
}

/// CSS Pseudo-elements L4 §3.4 — inheritance through the `::first-line` /
/// `::first-letter` fictional tag sequence.
///
/// The pseudo-element is the *parent* of the affected content, not a blanket
/// override of it: a descendant that specifies a property itself (`<b>`'s
/// `font-weight`, `<em>`'s `font-style`, an inline `style="color:…"`) keeps its
/// own value; only what it merely inherited comes from the pseudo-element.
/// Replacing the whole style instead silently drops those inner declarations.
///
/// - `own` — the fragment's/segment's computed style (the descendant);
/// - `base` — the originating element's style, which `own` inherited from;
/// - `pseudo` — the `::first-line` / `::first-letter` style.
///
/// A property is taken from `pseudo` only when `own` still equals `base` for it,
/// i.e. nothing in the inline chain specified it. Only the properties that apply
/// to these pseudo-elements (§3.2 / §4.4) and are meaningful for a text run are
/// merged — box-level ones (background, margins) are painted from the
/// pseudo-element's own box, not from the fragment.
///
/// Approximation: a descendant that *re-declares* the originating element's own
/// value (`color: blue` inside a `color: blue` block) is indistinguishable from
/// plain inheritance here and loses to the pseudo-element.
pub fn merge_pseudo_inherited(
    own: &ComputedStyle,
    base: &ComputedStyle,
    pseudo: &ComputedStyle,
) -> ComputedStyle {
    let mut out = own.clone();
    // `own == base` for a property ⇒ it was inherited ⇒ the pseudo-element
    // supplies it. Split by `Copy`-ness: `clone()` on a `Copy` field would trip
    // `clippy::clone_on_copy`.
    macro_rules! take_copy {
        ($($f:ident),+ $(,)?) => { $(if out.$f == base.$f { out.$f = pseudo.$f; })+ };
    }
    macro_rules! take_clone {
        ($($f:ident),+ $(,)?) => { $(if out.$f == base.$f { out.$f = pseudo.$f.clone(); })+ };
    }
    take_copy!(
        color,
        color_space,
        font_size,
        line_height,
        line_height_is_relative,
        font_style,
        font_weight,
        font_variant_caps,
        font_variant_emoji,
        font_stretch,
        font_optical_sizing,
        font_size_adjust,
        text_transform,
        letter_spacing,
        word_spacing,
        text_decoration_line,
        text_decoration_style,
        text_decoration_thickness,
        text_decoration_skip_ink,
        text_emphasis_position,
        vertical_align,
    );
    take_clone!(
        font_family,
        font_variation_settings,
        font_feature_settings,
        font_palette,
        font_palette_resolved,
        text_decoration_color,
        text_emphasis_style,
        text_emphasis_color,
        text_shadow,
    );
    out
}

/// Вычисляет стиль для псевдоэлемента `::before` или `::after` элемента `node`.
///
/// `pseudo` — "before" или "after" (без "::"). `dark_mode` forwarded to
/// `@media (prefers-color-scheme: dark)` matching.
///
/// Возвращает `None` если:
/// - нет CSS-правил для данного псевдоэлемента на этом узле, или
/// - вычисленный `content` равен `none` / `normal`.
pub fn compute_pseudo_element_style(
    doc: &Document,
    node: NodeId,
    pseudo: &str,
    sheet: &Stylesheet,
    parent: &ComputedStyle,
    viewport: Size,
    dark_mode: bool,
) -> Option<ComputedStyle> {
    if !matches!(doc.get(node).data, NodeData::Element { .. }) {
        return None;
    }
    // BUG-341 S20 census hook — see `PseudoCascadeStats`.
    if !PSEUDO_STATS_ON.load(std::sync::atomic::Ordering::Relaxed) {
        return compute_pseudo_element_style_inner(doc, node, pseudo, sheet, parent, viewport, dark_mode);
    }
    let t_pseudo = std::time::Instant::now();
    let out = compute_pseudo_element_style_inner(doc, node, pseudo, sheet, parent, viewport, dark_mode);
    note_pseudo_cascade(pseudo, t_pseudo.elapsed().as_nanos() as u64, out.is_some());
    out
}

/// The body of [`compute_pseudo_element_style`] — split out so the census hook
/// above covers every exit path.
#[allow(clippy::too_many_arguments)]
fn compute_pseudo_element_style_inner(
    doc: &Document,
    node: NodeId,
    pseudo: &str,
    sheet: &Stylesheet,
    parent: &ComputedStyle,
    viewport: Size,
    dark_mode: bool,
) -> Option<ComputedStyle> {
    let _prof = lumen_core::profile::scope_detail("pseudo_style");

    // BUG-341 S23: if the sheet never uses this pseudo-element as a selector
    // subject, `matches_complex_for_pseudo` below cannot match anything and the
    // whole cascade is a no-op. `::marker` is the one exception — it
    // synthesizes a style out of `list-style-type` with no rule at all (CSS
    // Lists L3 §2.1, the `matched.is_empty()` branch), so it must never be
    // short-circuited here.
    if !pseudo.eq_ignore_ascii_case("marker")
        && !sheet_targets_pseudo(sheet, viewport, dark_mode, pseudo)
    {
        return None;
    }

    // Собираем matching declarations из всех правил.
    //
    // BUG-284: candidate pre-filter via the same thread-local `CascadeIndex` as
    // `compute_style` (subject-key bucketing is agnostic to `::before`/`::after`
    // being appended to the subject compound, so the same index is valid here).
    // This function runs for *every* element for both "before" and "after" —
    // unlike `compute_style`, it was never indexed at all, making it one of the
    // largest un-indexed cascade costs on stylesheets with many `@media` rules.
    //
    // BUG-341 S10: matching runs *before* `pseudo_inherited_style` — see that
    // function's doc comment. Nothing here reads the pseudo-element's own style.
    let prof_match = lumen_core::profile::scope_detail("ps_match");
    let mut matched: Vec<(bool, Specificity, usize, usize, &Declaration)> = Vec::new();
    let node_data = doc.get(node);
    let node_tag = node_data.element_name().map_or("", |q| q.local.as_str());
    let node_id = node_data.get_attr("id");
    let class_attr = node_data.get_attr("class").unwrap_or("");
    let node_classes: Vec<&str> = class_attr.split_whitespace().collect();
    ensure_cascade_index(sheet, viewport, dark_mode);
    let cands = with_front_cascade_index(|idx| {
        idx.rules.candidates(node_tag, node_id, &node_classes)
    });
    for rule_idx in cands {
        let rule = &sheet.rules[rule_idx];
        let mut best: Option<Specificity> = None;
        for complex in &rule.selectors {
            if let Some(spec) = matches_complex_for_pseudo(complex, pseudo, doc, node) {
                best = Some(match best {
                    Some(prev) if prev >= spec => prev,
                    _ => spec,
                });
            }
        }
        if let Some(spec) = best {
            for (decl_idx, decl) in rule.declarations.iter().enumerate() {
                matched.push((decl.important, spec, rule_idx, decl_idx, decl));
            }
        }
    }

    // Perf: see the analogous @media/@supports comments in `compute_style` —
    // "active" precomputed once per (sheet, viewport, dark_mode) rather than
    // re-evaluated on every element (this function runs twice per element,
    // for `::before` and `::after`).
    let active_media = with_front_cascade_index(|idx| idx.active_media.clone());
    let mut next_rule_idx = sheet.rules.len();
    for (media_i, media) in sheet.media_rules.iter().enumerate() {
        if !active_media[media_i] {
            next_rule_idx += media.rules.len();
            continue;
        }
        let media_cands = with_front_cascade_index(|idx| {
            idx.media[media_i].candidates(node_tag, node_id, &node_classes)
        });
        for rule_idx in media_cands {
            let rule = &media.rules[rule_idx];
            let mut best: Option<Specificity> = None;
            for complex in &rule.selectors {
                if let Some(spec) = matches_complex_for_pseudo(complex, pseudo, doc, node) {
                    best = Some(match best {
                        Some(prev) if prev >= spec => prev,
                        _ => spec,
                    });
                }
            }
            if let Some(spec) = best {
                let global_rule_idx = next_rule_idx + rule_idx;
                for (decl_idx, decl) in rule.declarations.iter().enumerate() {
                    matched.push((decl.important, spec, global_rule_idx, decl_idx, decl));
                }
            }
        }
        next_rule_idx += media.rules.len();
    }
    // CSS Conditional Rules L3 §2 — @supports in pseudo-element context.
    let active_supports = with_front_cascade_index(|idx| idx.active_supports.clone());
    for (supports_i, supports) in sheet.supports_rules.iter().enumerate() {
        if !active_supports[supports_i] {
            next_rule_idx += supports.rules.len();
            continue;
        }
        let supports_cands = with_front_cascade_index(|idx| {
            idx.supports[supports_i].candidates(node_tag, node_id, &node_classes)
        });
        for rule_idx in supports_cands {
            let rule = &supports.rules[rule_idx];
            let mut best: Option<Specificity> = None;
            for complex in &rule.selectors {
                if let Some(spec) = matches_complex_for_pseudo(complex, pseudo, doc, node) {
                    best = Some(match best {
                        Some(prev) if prev >= spec => prev,
                        _ => spec,
                    });
                }
            }
            if let Some(spec) = best {
                let global_rule_idx = next_rule_idx + rule_idx;
                for (decl_idx, decl) in rule.declarations.iter().enumerate() {
                    matched.push((decl.important, spec, global_rule_idx, decl_idx, decl));
                }
            }
        }
        next_rule_idx += supports.rules.len();
    }

    if matched.is_empty() {
        // CSS Lists L3 §2.1: ::marker always generates a marker box from list-style-type
        // without any explicit CSS rule. Other pseudo-elements require a matching declaration.
        if pseudo.eq_ignore_ascii_case("marker") {
            let _prof_init = lumen_core::profile::scope_detail("ps_init");
            return Some(pseudo_inherited_style(parent));
        }
        return None;
    }
    drop(prof_match);
    let mut style = {
        let _prof_init = lumen_core::profile::scope_detail("ps_init");
        pseudo_inherited_style(parent)
    };
    let _prof_apply = lumen_core::profile::scope_detail("ps_apply");

    matched.sort_by_key(|&(imp, spec, rule_idx, decl_idx, _)| (imp, spec, rule_idx, decl_idx));

    let parent_fs = parent.font_size;
    let is_quirks = doc.mode() == DocumentMode::Quirks;
    for (_, _, _, _, decl) in &matched {
        // Pseudo-element style: the basis is irrelevant here — `zoom` is folded
        // into the originating element's style, which this one inherits from.
        let _ = apply_font_size(&mut style, decl, parent_fs, parent_fs, viewport, is_quirks);
    }
    let em_basis = style.font_size;
    let parent_weight = parent.font_weight;
    // CSS Pseudo-Elements L4 §5.5 — only a restricted set of properties applies to
    // `::marker`. Declarations outside that set (e.g. `line-height`, `margin`,
    // `background`) are dropped so a `::marker` rule cannot perturb marker layout
    // or paint beyond the spec-permitted font/color/text-flow styling.
    let is_marker = pseudo.eq_ignore_ascii_case("marker");
    for (_, _, _, _, decl) in &matched {
        if is_marker && !marker_property_applies(&decl.property) {
            continue;
        }
        let attr_buf;
        let effective_decl: &Declaration = if decl.value.contains("attr(") {
            let Some(v) = expand_attr_val(&decl.value, doc, node) else { continue };
            attr_buf = Declaration { property: decl.property.clone(), value: v, important: decl.important };
            &attr_buf
        } else {
            decl
        };
        apply_declaration(&mut style, effective_decl, em_basis, viewport, parent_weight, parent, parent, is_quirks, dark_mode);
    }

    // ::before/::after require content: to render; ::first-letter/::first-line do not.
    // ::marker renders by default (content comes from list-style-type); content:none suppresses it.
    // ::selection applies to active text selection — no content required (CSS Pseudo-elements L4 §5.6).
    // ::placeholder styles the UA-generated placeholder hint text — no content required
    // (CSS Pseudo-elements L4 §4.10).
    // CC-CSS-1: `::-webkit-scrollbar`/`-thumb`/`-track` are legacy scrollbar-styling
    // pseudo-elements (translated onto `scrollbar-width`/`scrollbar-color` by
    // `apply_webkit_scrollbar_pseudos`) — no `content:` required either.
    if pseudo.eq_ignore_ascii_case("first-letter")
        || pseudo.eq_ignore_ascii_case("first-line")
        || pseudo.eq_ignore_ascii_case("selection")
        || pseudo.eq_ignore_ascii_case("placeholder")
        || pseudo.eq_ignore_ascii_case("-webkit-scrollbar")
        || pseudo.eq_ignore_ascii_case("-webkit-scrollbar-thumb")
        || pseudo.eq_ignore_ascii_case("-webkit-scrollbar-track")
    {
        Some(style)
    } else if pseudo.eq_ignore_ascii_case("marker") {
        match &style.content {
            Content::None => None,
            _ => Some(style),
        }
    } else {
        match &style.content {
            Content::Items(_) => Some(style),
            _ => None,
        }
    }
}

/// Computes the `::selection` override style for a DOM element.
///
/// Collects all CSS rules targeting `element::selection`, applies declarations
/// in specificity order, and returns the computed style. Returns `None` when
/// no `::selection` rules match `node` (callers should fall back to the OS
/// default selection highlight colour in that case).
///
/// Only a limited subset of properties are honoured by `::selection` per
/// CSS Pseudo-elements L4 §5.6: `color`, `background-color`,
/// `text-decoration-*`, `text-shadow`. Other declared properties are parsed
/// and stored but should be ignored by the paint layer.
pub fn compute_selection_style(
    doc: &Document,
    node: NodeId,
    sheet: &Stylesheet,
    parent: &ComputedStyle,
    viewport: Size,
    dark_mode: bool,
) -> Option<ComputedStyle> {
    compute_pseudo_element_style(doc, node, "selection", sheet, parent, viewport, dark_mode)
}
