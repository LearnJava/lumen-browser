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
use crate::style::cascade::{part_scopes_of, set_part_scope_host};
use crate::style::matches_part_complex;
use crate::style::{
    apply_declaration, apply_font_size, ensure_cascade_index, expand_attr_val, matches_complex,
    note_pseudo_cascade, sheet_targets_pseudo, with_front_cascade_index, ComputedStyle, Content,
    ContentItem,
    Display, PSEUDO_STATS_ON, SHADOW_HOST_SCOPE,
};
use lumen_core::geom::Size;
use lumen_css_parser::{
    ComplexSelector, CompoundSelector, Declaration, PseudoElementKind, SimpleSelector, Specificity,
    Stylesheet,
};
use lumen_dom::{Document, DocumentMode, NodeData, NodeId};

/// Rule-order base of `host::part()::pseudo` rules, above every part-less rule of the sheet.
const PART_RULE_BASE: usize = usize::MAX / 2;

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
        PseudoElementKind::Part(_) => "part",
        PseudoElementKind::Marker => "marker",
        PseudoElementKind::Selection => "selection",
        PseudoElementKind::TargetText => "target-text",
        PseudoElementKind::SpellingError => "spelling-error",
        PseudoElementKind::GrammarError => "grammar-error",
        PseudoElementKind::Placeholder => "placeholder",
        PseudoElementKind::Highlight(_) => "highlight",
        PseudoElementKind::Picker(_) => "picker",
        PseudoElementKind::Checkmark => "checkmark",
        PseudoElementKind::PickerIcon => "picker-icon",
        PseudoElementKind::ScrollMarker => "scroll-marker",
        PseudoElementKind::ScrollMarkerGroup => "scroll-marker-group",
        PseudoElementKind::Column => "column",
        PseudoElementKind::ScrollButton(_) => "scroll-button",
        PseudoElementKind::ViewTransition => "view-transition",
        PseudoElementKind::ViewTransitionGroup(_) => "view-transition-group",
        PseudoElementKind::ViewTransitionImagePair(_) => "view-transition-image-pair",
        PseudoElementKind::ViewTransitionOld(_) => "view-transition-old",
        PseudoElementKind::ViewTransitionNew(_) => "view-transition-new",
        PseudoElementKind::Unknown(s) => s.as_str(),
    }
}

/// Splits a pseudo-element request into its bare name and, for the
/// functional forms the caller asks about by argument, that argument:
/// `"view-transition-old(hero)"` → `("view-transition-old", Some("hero"))`.
fn split_pseudo_request(pseudo: &str) -> (&str, Option<&str>) {
    match pseudo.split_once('(') {
        Some((base, rest)) => (base, Some(rest.strip_suffix(')').unwrap_or(rest))),
        None => (pseudo, None),
    }
}

/// True for the highlight pseudo-elements (CSS Pseudo-Elements L4 §3): `selection`,
/// `target-text`, `spelling-error`, `grammar-error` and `highlight(<name>)`. They style
/// a stretch of text, generate no box and need no `content`.
pub fn is_highlight_pseudo(pseudo: &str) -> bool {
    let base = split_pseudo_request(pseudo).0;
    ["selection", "target-text", "spelling-error", "grammar-error", "highlight"]
        .iter()
        .any(|n| base.eq_ignore_ascii_case(n))
}

/// Helper: check if a pseudo-element name matches a PseudoElementKind.
///
/// `name` is either a bare name (`"before"`) or, for the `::view-transition-*`
/// family, `"<name>(<ident>)"`: the selector's own argument must then be that
/// ident (case-sensitive, it is a `<custom-ident>`) or the `*` wildcard (CSS
/// View Transitions L1 §6.2–6.5). A bare `"view-transition-group"` matches any
/// argument. Other functional kinds keep the old contract — name only.
fn pseudo_element_matches(kind: &PseudoElementKind, name: &str) -> bool {
    let (base, wanted) = split_pseudo_request(name);
    if !pseudo_element_name(kind).eq_ignore_ascii_case(base) {
        return false;
    }
    // `::scroll-button(<dir>)`: the request lists every spelling the box answers to,
    // `"scroll-button(block-end|down)"` (logical + physical for the originating element's
    // writing mode); the selector's argument is matched against any of them, `*` against all.
    if let PseudoElementKind::ScrollButton(dir) = kind {
        return match wanted {
            None => true,
            Some(w) => dir == "*" || w.split('|').any(|alias| alias == dir),
        };
    }
    // `::highlight(<name>)` (CSS Custom Highlight API L1 §3.1): a `<custom-ident>`,
    // case-sensitive; a bare `"highlight"` request matches any name.
    if let PseudoElementKind::Highlight(own) = kind {
        return wanted.is_none_or(|w| own == w);
    }
    let own = match kind {
        PseudoElementKind::ViewTransitionGroup(a)
        | PseudoElementKind::ViewTransitionImagePair(a)
        | PseudoElementKind::ViewTransitionOld(a)
        | PseudoElementKind::ViewTransitionNew(a) => a.as_str(),
        _ => return true,
    };
    wanted.is_none_or(|w| own == "*" || own == w)
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
    // `host::part(a)::before` styles a part of the host's shadow tree, never the host itself:
    // `compute_pseudo_element_style_inner` matches it against the part element.
    if last.parts.iter().any(|p| matches!(p, SimpleSelector::PseudoElement(PseudoElementKind::Part(_)))) {
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
/// `text-combine-upright` writing-mode properties, `content`, the text-flow properties the
/// WPT `marker-*` tests exercise (`text-transform`, `letter-spacing`, `word-spacing`,
/// `text-shadow`, `text-emphasis*`, `tab-size`, `hyphens`, wrapping), and all animation
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
        || p.starts_with("text-emphasis")
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
                | "text-transform"
                | "letter-spacing"
                | "word-spacing"
                | "text-shadow"
                | "tab-size"
                | "hyphens"
                | "line-break"
                | "word-break"
                | "overflow-wrap"
                | "word-wrap"
        )
}

/// CSS Pseudo-Elements L4 §3.2 — true when `prop` applies to `::first-line`: the font
/// properties, `color`, `opacity`, the background properties, `text-decoration*` /
/// `text-emphasis*` / `text-underline-*` / `text-shadow` / `text-transform` / `text-justify`,
/// `-webkit-text-*`, `letter-spacing`, `word-spacing`, `line-height` and `vertical-align`.
/// Every other declaration is dropped; custom properties stay for `var()`.
fn first_line_property_applies(prop: &str) -> bool {
    let p = prop.trim().to_ascii_lowercase();
    p.starts_with("--")
        || p.starts_with("font")
        || p.starts_with("background")
        || p.starts_with("text-decoration")
        || p.starts_with("text-emphasis")
        || p.starts_with("text-underline")
        || p.starts_with("-webkit-text-")
        || matches!(
            p.as_str(),
            "color"
                | "opacity"
                | "letter-spacing"
                | "word-spacing"
                | "line-height"
                | "text-shadow"
                | "text-transform"
                | "text-justify"
                | "vertical-align"
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
/// The style of a highlight pseudo-element that matches no rule of its own: what it
/// inherits from the highlight of the parent element (CSS Pseudo-Elements L4 §3.3).
pub(crate) fn highlight_inherited_style(parent: &ComputedStyle) -> ComputedStyle {
    pseudo_inherited_style(parent)
}

/// CSS Pseudo-Elements L4 §4.2 — the UA style of `::marker` on top of the inherited base:
/// `unicode-bidi: isolate`, `font-variant-numeric: tabular-nums`, `text-transform: none`,
/// `text-indent: 0`. The last two are inherited properties that a marker must *not* take
/// from its list item (BUG-1446).
fn marker_base_style(parent: &ComputedStyle) -> ComputedStyle {
    use crate::style::NumericSpacing;
    use crate::style::{TextTransform, TextTransformExtra, UnicodeBidi};
    let mut style = pseudo_inherited_style(parent);
    style.unicode_bidi = UnicodeBidi::Isolate;
    style.font_variant_numeric.spacing = NumericSpacing::Tabular;
    style.text_transform = TextTransform::None;
    style.text_extra.transform = TextTransformExtra::default();
    style.text_extra.indent_hanging = false;
    style.text_extra.indent_each_line = false;
    style.text_indent = crate::style::Length::Px(0.0);
    style
}

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
    style.color_css = parent.color_css.clone();
    style.text_align = parent.text_align;
    style.direction = parent.direction;
    style.font_size = parent.font_size;
    style.line_height = parent.line_height;
    style.line_height_is_relative = parent.line_height_is_relative;
    style.line_height_is_normal = parent.line_height_is_normal;
    style.line_height_step = parent.line_height_step;
    style.font_style = parent.font_style;
    style.font_oblique_angle = parent.font_oblique_angle;
    style.font_weight = parent.font_weight;
    style.font_variant_caps = parent.font_variant_caps;
    style.font_variant_emoji = parent.font_variant_emoji;
    style.font_variant_ligatures = parent.font_variant_ligatures;
    style.font_variant_numeric = parent.font_variant_numeric;
    style.font_variant_position = parent.font_variant_position;
    style.font_stretch = parent.font_stretch;
    style.font_family = parent.font_family.clone();
    style.font_variation_settings = parent.font_variation_settings.clone();
    style.font_feature_settings = parent.font_feature_settings.clone();
    style.font_palette = parent.font_palette.clone();
    style.font_palette_resolved = parent.font_palette_resolved.clone();
    style.text_transform = parent.text_transform;
    style.text_extra = parent.text_extra;
    style.white_space = parent.white_space;
    style.white_space_collapse = parent.white_space_collapse;
    style.text_indent = parent.text_indent.clone();
    style.letter_spacing = parent.letter_spacing;
    style.letter_spacing_pct = parent.letter_spacing_pct;
    style.word_spacing = parent.word_spacing;
    style.word_spacing_pct = parent.word_spacing_pct;
    style.text_decoration_line = parent.text_decoration_line;
    style.text_decoration_color = parent.text_decoration_color;
    style.text_decoration_style = parent.text_decoration_style;
    style.text_decoration_thickness = parent.text_decoration_thickness;
    style.text_emphasis_style = parent.text_emphasis_style.clone();
    style.text_emphasis_color = parent.text_emphasis_color;
    style.text_fill_color = parent.text_fill_color;
    style.text_stroke_width = parent.text_stroke_width;
    style.text_stroke_color = parent.text_stroke_color;
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
    style.tab_size = parent.tab_size;
    style.caret_color = parent.caret_color;
    style.caret_shape = parent.caret_shape;
    style.caret_animation = parent.caret_animation;
    style.interactivity = parent.interactivity;
    style.input_security = parent.input_security;
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
        line_height_is_normal,
        font_style,
        font_oblique_angle,
        font_weight,
        font_variant_caps,
        font_variant_emoji,
        font_variant_ligatures,
        font_variant_numeric,
        font_variant_position,
        font_stretch,
        font_optical_sizing,
        font_size_adjust,
        text_transform,
        text_extra,
        letter_spacing,
        word_spacing,
        text_decoration_line,
        text_decoration_style,
        text_decoration_thickness,
        text_decoration_skip_ink,
        text_emphasis_position,
        text_stroke_width,
        vertical_align,
    );
    take_clone!(
        color_css,
        font_family,
        font_variation_settings,
        font_feature_settings,
        font_palette,
        font_palette_resolved,
        text_decoration_color,
        text_emphasis_style,
        text_emphasis_color,
        text_fill_color,
        text_stroke_color,
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

/// UA-sheet `content` of `q::before` / `q::after` (HTML rendering §15.3.7). Author rules
/// override it through the ordinary cascade, so it only seeds the pseudo-element's style.
pub fn ua_quote_content(doc: &Document, node: NodeId, pseudo: &str) -> Option<ContentItem> {
    let is_q = doc.get(node).element_name().is_some_and(|q| q.local.as_str() == "q");
    if !is_q {
        return None;
    }
    if pseudo.eq_ignore_ascii_case("before") {
        Some(ContentItem::OpenQuote)
    } else if pseudo.eq_ignore_ascii_case("after") {
        Some(ContentItem::CloseQuote)
    } else {
        None
    }
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
    // CSS Color L5 §5.3 — see `compute_style_shareable`.
    crate::style::parse::color::sync_color_profiles(sheet);

    // BUG-341 S23: if the sheet never uses this pseudo-element as a selector
    // subject, `matches_complex_for_pseudo` below cannot match anything and the
    // whole cascade is a no-op. `::marker` is the one exception — it
    // synthesizes a style out of `list-style-type` with no rule at all (CSS
    // Lists L3 §2.1, the `matched.is_empty()` branch), so it must never be
    // short-circuited here.
    // HTML rendering §15.3.7: `q::before`/`q::after` get `open-quote`/`close-quote` from the UA
    // sheet (BUG-1598) — no author rule needed, so the sheet-level gate does not apply.
    let ua_quote = ua_quote_content(doc, node, pseudo);
    // CSS Shadow Parts L1 §2: `host::part(a)::before` rules of the outer trees reach a `part` element.
    let part_scopes = part_scopes_of(doc, node);
    let is_scroll_group = pseudo.eq_ignore_ascii_case("scroll-marker-group");
    if !pseudo.eq_ignore_ascii_case("marker")
        && !is_scroll_group
        && ua_quote.is_none()
        && part_scopes.is_empty()
        && !sheet_targets_pseudo(sheet, viewport, dark_mode, split_pseudo_request(pseudo).0)
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
    let node_attrs: &[lumen_dom::Attribute] = match &node_data.data {
        lumen_dom::NodeData::Element { attrs, .. } => attrs,
        _ => &[],
    };
    ensure_cascade_index(sheet, viewport, dark_mode);
    // BUG-935 срез 90: candidates come from the pseudo-subject indices — rules without a
    // pseudo-element in a subject cannot match here (`matches_complex_for_pseudo` returns
    // `None` for them before it looks at the node), and a `@media`/`@supports` block with no
    // such rule is not visited at all. Indices are the general index's own (same numbering).
    let (cands, style_keys) = with_front_cascade_index(|idx| {
        let cands = idx.pseudo_rules.candidates(node_tag, node_id, &node_classes, node_attrs);
        let keys = cands.iter().map(|&i| idx.order.style(i)).collect::<Vec<_>>();
        (cands, keys)
    });
    for (rule_idx, key) in cands.into_iter().zip(style_keys) {
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
                matched.push((decl.important, spec, key, decl_idx, decl));
            }
        }
    }

    // Perf: see the analogous @media/@supports comments in `compute_style` —
    // "active" precomputed once per (sheet, viewport, dark_mode) rather than
    // re-evaluated on every element (this function runs twice per element,
    // for `::before` and `::after`).
    // CSS Conditional Rules L3 §2 — @supports in pseudo-element context.
    with_front_cascade_index(|idx| {
        let blocks = idx
            .pseudo_media
            .iter()
            .filter(|b| idx.active_media[b.block])
            .map(|b| (b, &sheet.media_rules[b.block].rules))
            .chain(
                idx.pseudo_supports
                    .iter()
                    .filter(|b| idx.active_supports[b.block])
                    .map(|b| (b, &sheet.supports_rules[b.block].rules)),
            );
        for (block, rules) in blocks {
            for rule_idx in block.index.candidates(node_tag, node_id, &node_classes, node_attrs) {
                let rule = &rules[rule_idx];
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
                    let global_rule_idx = block.base + rule_idx;
                    for (decl_idx, decl) in rule.declarations.iter().enumerate() {
                        matched.push((decl.important, spec, global_rule_idx, decl_idx, decl));
                    }
                }
            }
        }
    });

    // Part scopes: the outer tree's `host::part(names)::pseudo` rules match the part element. They sort
    // after the part-less rules of their sheet (rule index offset), never ahead by specificity.
    let part_pseudo_ok = |k: &PseudoElementKind| pseudo_element_matches(k, pseudo);
    for scope in &part_scopes {
        let names: Vec<&str> = scope.names.iter().map(String::as_str).collect();
        set_part_scope_host(doc, scope);
        let rules: &[lumen_css_parser::Rule] = match &scope.sheet {
            Some(shadow) => &shadow.rules,
            None => &sheet.rules,
        };
        for (i, rule) in rules.iter().enumerate() {
            let mut best: Option<Specificity> = None;
            for complex in &rule.selectors {
                if let Some(spec) = matches_part_complex(complex, doc, node, scope.host, &names, Some(&part_pseudo_ok)) {
                    best = Some(match best {
                        Some(prev) if prev >= spec => prev,
                        _ => spec,
                    });
                }
            }
            if let Some(spec) = best {
                for (decl_idx, decl) in rule.declarations.iter().enumerate() {
                    matched.push((decl.important, spec, PART_RULE_BASE + i, decl_idx, decl));
                }
            }
        }
        SHADOW_HOST_SCOPE.with(|c| c.set(u32::MAX));
    }

    if matched.is_empty() {
        // CSS Lists L3 §2.1: ::marker always generates a marker box from list-style-type
        // without any explicit CSS rule. Other pseudo-elements require a matching declaration.
        if pseudo.eq_ignore_ascii_case("marker") {
            let _prof_init = lumen_core::profile::scope_detail("ps_init");
            return Some(marker_base_style(parent));
        }
        if let Some(item) = ua_quote {
            let mut style = pseudo_inherited_style(parent);
            style.content = Content::Items(vec![item]);
            return Some(style);
        }
        // CSS Overflow L5 §scroll-marker-group: an originating element whose
        // `scroll-marker-group` is not `none` gets the group box even with no author rule.
        if is_scroll_group {
            let mut style = pseudo_inherited_style(parent);
            style.display = Display::Block;
            return Some(style);
        }
        return None;
    }
    drop(prof_match);
    let mut style = {
        let _prof_init = lumen_core::profile::scope_detail("ps_init");
        let mut style = if pseudo.eq_ignore_ascii_case("marker") { marker_base_style(parent) } else { pseudo_inherited_style(parent) };
        if let Some(item) = ua_quote {
            style.content = Content::Items(vec![item]);
        }
        if is_scroll_group {
            style.display = Display::Block;
        }
        if split_pseudo_request(pseudo).0.eq_ignore_ascii_case("scroll-button") {
            super::ua::apply_ua_scroll_button(&mut style, dark_mode);
        }
        style
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
    // The same for `::first-line` (§3.2): a box-level declaration (`margin`, `position`,
    // `word-break`, ...) does not apply to it, so the computed value is the parent's (BUG-1437).
    let is_first_line = pseudo.eq_ignore_ascii_case("first-line");
    for (_, _, _, _, decl) in &matched {
        if (is_marker && !marker_property_applies(&decl.property))
            || (is_first_line && !first_line_property_applies(&decl.property))
        {
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
    // CSS Overflow L5 §scroll-marker-group: the group box needs no `content` (it is a container
    // for the markers, not generated content); `::scroll-marker`/`::scroll-button()` do.
    if pseudo.eq_ignore_ascii_case("first-letter")
        || pseudo.eq_ignore_ascii_case("scroll-marker-group")
        || pseudo.eq_ignore_ascii_case("first-line")
        || is_highlight_pseudo(pseudo)
        || pseudo.eq_ignore_ascii_case("placeholder")
        || pseudo.eq_ignore_ascii_case("-webkit-scrollbar")
        || pseudo.eq_ignore_ascii_case("-webkit-scrollbar-thumb")
        || pseudo.eq_ignore_ascii_case("-webkit-scrollbar-track")
        || split_pseudo_request(pseudo).0.starts_with("view-transition")
    {
        // CSS View Transitions L1 §6: the `::view-transition-*` tree is built
        // by the UA, not by `content:` — no `content` required.
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

/// Computes the `::target-text` override style for a DOM element (CSS
/// Pseudo-Elements L4 §5.7 / WICG Scroll To Text Fragment §4) — text matched
/// by a `:~:text=` URL fragment directive. Same restricted property subset
/// and `None`-means-fall-back-to-UA-default contract as [`compute_selection_style`].
pub fn compute_target_text_style(
    doc: &Document,
    node: NodeId,
    sheet: &Stylesheet,
    parent: &ComputedStyle,
    viewport: Size,
    dark_mode: bool,
) -> Option<ComputedStyle> {
    compute_pseudo_element_style(doc, node, "target-text", sheet, parent, viewport, dark_mode)
}

/// Which part of the `::view-transition` pseudo-tree a rule targets (CSS View
/// Transitions L1 §6).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewTransitionPart {
    /// `::view-transition-group(<name>)`.
    Group,
    /// `::view-transition-image-pair(<name>)`.
    ImagePair,
    /// `::view-transition-old(<name>)`.
    Old,
    /// `::view-transition-new(<name>)`.
    New,
}

impl ViewTransitionPart {
    fn request(self, name: &str) -> String {
        let base = match self {
            Self::Group => "view-transition-group",
            Self::ImagePair => "view-transition-image-pair",
            Self::Old => "view-transition-old",
            Self::New => "view-transition-new",
        };
        format!("{base}({name})")
    }
}

/// Computes the author style of one `::view-transition-*` pseudo-element for
/// the capture named `name` (`"root"` for the whole-page capture).
///
/// The pseudo-tree originates from the document element, so `doc`'s root
/// element is the node the rules' originating compound is matched against
/// (`:root::view-transition-old(root)` and the bare
/// `::view-transition-old(root)` both match; `.x::view-transition-old(root)`
/// matches only when the root element has that class). Rules whose argument
/// is `*` apply to every name.
///
/// `None` when no rule targets this part/name — the caller keeps the UA
/// default (the shell's 300 ms linear fade). The pseudo-tree has no real
/// parent style available here, so inherited properties come from the initial
/// style: only the non-inherited `animation-*` longhands are meaningful to
/// callers today.
pub fn compute_view_transition_pseudo_style(
    doc: &Document,
    sheet: &Stylesheet,
    part: ViewTransitionPart,
    name: &str,
    viewport: Size,
    dark_mode: bool,
) -> Option<ComputedStyle> {
    let root = doc.document_element()?;
    compute_pseudo_element_style(
        doc,
        root,
        &part.request(name),
        sheet,
        &ComputedStyle::root(),
        viewport,
        dark_mode,
    )
}
