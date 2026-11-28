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
    matching::matches_simple, note_pseudo_cascade, sheet_targets_pseudo, with_front_cascade_index,
    ComputedStyle, Content, ContentItem, Display, PSEUDO_STATS_ON, SHADOW_HOST_SCOPE,
};
use lumen_core::geom::Size;
use lumen_css_parser::{
    ComplexSelector, CompoundSelector, Declaration, PseudoClass, PseudoElementKind, SimpleSelector,
    Specificity, Stylesheet,
};
use lumen_dom::{Document, DocumentMode, NodeData, NodeId};

/// Rule-order base of `host::part()::pseudo` rules, above every part-less rule of the sheet.
const PART_RULE_BASE: usize = usize::MAX / 2;

/// PERF-PSEUDO-PRE — one declaration a pseudo-element rule contributed to a
/// node's cascade, as an owned record. `style::pre_match` carries these across
/// rayon workers; the sequential walk resolves them back to `&Declaration`
/// through [`resolve_pseudo_decl`] — the same declarations the candidate scan
/// would have produced, minus the scan itself.
#[derive(Clone, Copy, Debug)]
pub(crate) struct PseudoMatch {
    /// `Declaration::important` — first cascade-sort key.
    pub important: bool,
    /// The rule's best-matching selector specificity.
    pub spec: Specificity,
    /// Source-order key — the numbering the declaration sort used
    /// (`RuleOrder::style`, `block.base + rule`, `PART_RULE_BASE + i`).
    pub key: usize,
    /// Index into the owning rule's `declarations`.
    pub decl_idx: usize,
    /// Which rule list owns the declaration — see [`PseudoRuleRef`].
    pub rule: PseudoRuleRef,
}

/// PERF-PSEUDO-PRE — one node's pseudo match records as `pre_match` hands them
/// to the pass's [`crate::counters::PseudoCache`]: `(pseudo base name,
/// cascade-sorted matches)` pairs per element.
pub(crate) type PseudoRecords = Vec<(NodeId, Vec<(Box<str>, std::sync::Arc<[PseudoMatch]>)>)>;

/// Where a [`PseudoMatch`]'s declaration lives, in the same numbering the
/// sequential matcher pushed it under.
#[derive(Clone, Copy, Debug)]
pub(crate) enum PseudoRuleRef {
    /// `sheet.rules[i]`.
    Top(u32),
    /// `sheet.media_rules[block].rules[i]` — `rule` is block-local.
    Media { block: u32, rule: u32 },
    /// `sheet.supports_rules[block].rules[i]` — `rule` is block-local.
    Supports { block: u32, rule: u32 },
    /// `part_scopes[scope]`'s rule list (its own sheet, or the document sheet
    /// when the scope has none), rule `i`.
    Part { scope: u32, rule: u32 },
}

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
    // BUG-1800 срез 8: subject-компаунд (последний, без PseudoElement-частей) против узла —
    // ДО построения модифицированного селектора. Прежний путь клонировал tail и все части
    // на каждый вызов; псевдо-пробы `counters::walk` дают ~37k вызовов на флаш pikabu при
    // ~99 % отказов. Части и порядок — сами `matches_compound`/`matches_simple` (пустой
    // компаунд — universal, не-элемент — отказ): результат совпадает с прежним бит-в-бит.
    let NodeData::Element { name, attrs } = &doc.get(node).data else {
        return None;
    };
    // CSS Overflow L5 §3.3.2: focus pseudo-classes on scroll-control
    // pseudo-elements (`::scroll-marker`, `::scroll-button()`,
    // `::scroll-marker-group`) match the pseudo-element focus state recorded
    // in `env::PSEUDO_FOCUS`, not the originating element's own `:focus*` —
    // which is always false while a pseudo-element holds focus anyway. They
    // are lifted out of the subject compound here and checked separately
    // below, so `matches_simple` never sees them.
    let pseudo_base = split_pseudo_request(pseudo).0;
    let is_marker = pseudo_base.eq_ignore_ascii_case("scroll-marker");
    let is_group = pseudo_base.eq_ignore_ascii_case("scroll-marker-group");
    let is_button = pseudo_base.eq_ignore_ascii_case("scroll-button");
    let scroll_control = is_marker || is_group || is_button;
    // Псевдоклассы, которые у scroll-control псевдоэлементов означают состояние
    // САМОГО псевдо-бокса, а не originating-элемента: focus-набор для всех трёх
    // (CSS Overflow L5 §3.3.2) и `:enabled`/`:disabled` — только для кнопок
    // (§3.2: «`:disabled` can apply to `::scroll-button()`»).
    let is_lifted_part = |p: &SimpleSelector| {
        scroll_control
            && match p {
                SimpleSelector::PseudoClass(
                    PseudoClass::Focus
                    | PseudoClass::FocusVisible
                    | PseudoClass::FocusWithin
                    | PseudoClass::Active,
                ) => true,
                SimpleSelector::PseudoClass(PseudoClass::Enabled | PseudoClass::Disabled) => is_button,
                // CSS Overflow L5 §3.1.7 — `:target-current`/`:target-before`/
                // `:target-after` описывают состояние самого `::scroll-marker`
                // в его группе, а не originating-элемента.
                SimpleSelector::PseudoClass(
                    PseudoClass::TargetCurrent | PseudoClass::TargetBefore | PseudoClass::TargetAfter,
                ) => is_marker,
                _ => false,
            }
    };
    if !last.parts.iter().all(|p| {
        matches!(p, SimpleSelector::PseudoElement(_))
            || is_lifted_part(p)
            || matches_simple(p, doc, node, &name.local, attrs)
    }) {
        return None;
    }
    // Собираем модифицированный последний compound без PseudoElement и без
    // поднятых псевдоклассов scroll-control псевдоэлементов (см. выше).
    let stripped = CompoundSelector {
        parts: last.parts.iter()
            .filter(|p| !matches!(p, SimpleSelector::PseudoElement(_)) && !is_lifted_part(p))
            .cloned()
            .collect(),
    };
    // Собираем модифицированный ComplexSelector. Односоставный селектор
    // (`::scroll-marker:focus` без предков) имеет пустой `tail` — его
    // subject-компаунд уже проверен выше инлайн, `modified` не нужен.
    let element_matched = if complex.tail.is_empty() {
        true
    } else {
        let mut tail = complex.tail.clone();
        tail.last_mut().unwrap().1 = stripped;
        matches_complex(&ComplexSelector { head: complex.head.clone(), tail }, doc, node)
    };
    if !element_matched {
        return None;
    }
    if scroll_control
        && !last.parts.iter().all(|p| {
            !is_lifted_part(p) || scroll_pseudo_part_matches(node, pseudo_base, pseudo, p)
        })
    {
        return None;
    }
    Some(complex.specificity())
}

/// CSS Overflow L5 §3.3.2/§3.2 — does lifted pseudo-class `p` (a
/// `SimpleSelector` already known to be one of the scroll-control state
/// classes) hold for the pseudo-element `pseudo_base` of `node`?
///
/// Focus classes read the pseudo-element focus recorded in `env::PSEUDO_FOCUS`:
/// - `::scroll-marker`/`::scroll-button()`: `:focus`/`:active`/`:focus-within`
///   match when THIS pseudo-element (origin + kind) is the focused one;
///   `:focus-visible` additionally requires keyboard focus modality.
/// - `::scroll-marker-group`: never `:focus` itself; `:focus-within` matches
///   while a marker of its group is focused (the group belongs to the scroll
///   container `node`).
///
/// `:enabled`/`:disabled` (only lifted for `::scroll-button()`) read the
/// per-direction scrollability installed by `box_tree::scroll_pseudos` for
/// the pseudo-element's own build (`scroll_state::install_scroll_button_states`).
fn scroll_pseudo_part_matches(
    node: NodeId,
    pseudo_base: &str,
    pseudo: &str,
    p: &SimpleSelector,
) -> bool {
    let is_group = pseudo_base.eq_ignore_ascii_case("scroll-marker-group");
    let is_marker = pseudo_base.eq_ignore_ascii_case("scroll-marker");
    let is_button = pseudo_base.eq_ignore_ascii_case("scroll-button");
    if !(is_group || is_marker || is_button) {
        return false;
    }
    let pc = match p {
        SimpleSelector::PseudoClass(pc) => pc,
        _ => return false,
    };
    match pc {
        PseudoClass::Enabled | PseudoClass::Disabled => {
            if !is_button {
                return false;
            }
            // Запрос собирается `button_request` как `"<logical>|<physical>"`;
            // физическое направление — состояние кнопки. Логический аргумент
            // без writing-mode здесь не разрешить — кнопка считается активной.
            let arg = split_pseudo_request(pseudo).1.unwrap_or("");
            let physical = arg.rsplit('|').next().unwrap_or(arg);
            let want_enabled = matches!(pc, PseudoClass::Enabled);
            match crate::scroll_state::scroll_button_enabled(node, physical) {
                Some(enabled) => enabled == want_enabled,
                None => want_enabled,
            }
        }
        // CSS Overflow L5 §3.1.7 — отношение `::scroll-marker` к активному
        // маркеру его группы приходит из `ScrollHints::markers` (посчитано по
        // прежнему дереву до пересборки).
        PseudoClass::TargetCurrent | PseudoClass::TargetBefore | PseudoClass::TargetAfter => {
            if !is_marker {
                return false;
            }
            let want = match pc {
                PseudoClass::TargetCurrent => crate::scroll_state::MARKER_CURRENT,
                PseudoClass::TargetBefore => crate::scroll_state::MARKER_BEFORE,
                _ => crate::scroll_state::MARKER_AFTER,
            };
            crate::scroll_state::scroll_marker_relation(node) == Some(want)
        }
        _ => scroll_pseudo_focus_matches(node, pseudo_base, p),
    }
}

/// CSS Overflow L5 §3.3.2 — does focus pseudo-class `p` (a `SimpleSelector`
/// already known to be `PseudoClass::Focus`/`FocusVisible`/`FocusWithin`/
/// `Active`) hold for the scroll-control pseudo-element `pseudo_base` of
/// `node`, given the pseudo-element focus recorded in `env::PSEUDO_FOCUS`?
fn scroll_pseudo_focus_matches(
    node: NodeId,
    pseudo_base: &str,
    p: &SimpleSelector,
) -> bool {
    let pf = crate::style::env::pseudo_focus();
    if !pf.is_some() {
        return false;
    }
    let is_group = pseudo_base.eq_ignore_ascii_case("scroll-marker-group");
    let is_marker = pseudo_base.eq_ignore_ascii_case("scroll-marker");
    let is_button = pseudo_base.eq_ignore_ascii_case("scroll-button");
    if !(is_group || is_marker || is_button) {
        return false;
    }
    // PseudoKind discriminants — same order as `PseudoHit`/`pseudo_kind_from_u8`.
    let kind_ok = if is_marker {
        pf.kind == crate::PseudoKind::ScrollMarker as u8
    } else if is_group {
        // Группа никогда не фокусируется сама, но её `:focus-within` живёт,
        // пока фокус держит `::scroll-marker` её группы (kind=ScrollMarker) —
        // пускать оба kind, точное pseudo-class решение ниже.
        pf.kind == crate::PseudoKind::ScrollMarkerGroup as u8
            || pf.kind == crate::PseudoKind::ScrollMarker as u8
    } else {
        (crate::PseudoKind::ScrollButtonBlockStart as u8
            ..=crate::PseudoKind::ScrollButtonBlockEnd as u8)
            .contains(&pf.kind)
    };
    if !kind_ok {
        return false;
    }
    let pc = match p {
        SimpleSelector::PseudoClass(pc) => pc,
        _ => return false,
    };
    match pc {
        // The group box itself is never the focused control.
        PseudoClass::Focus | PseudoClass::Active if is_group => false,
        PseudoClass::Focus | PseudoClass::Active => pf.origin == node.index() as u32,
        PseudoClass::FocusVisible => {
            !is_group
                && pf.origin == node.index() as u32
                && crate::style::env::keyboard_focus()
        }
        PseudoClass::FocusWithin => {
            if is_group {
                // The group belongs to the scroll container — `:focus-within`
                // matches while any of its markers holds focus.
                pf.container == node.index() as u32
            } else {
                // A focused pseudo-element matches `:focus-within` itself.
                pf.origin == node.index() as u32
            }
        }
        _ => false,
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
    style.text_underline_offset_pct = parent.text_underline_offset_pct;
    style.text_decoration_skip_spaces = parent.text_decoration_skip_spaces;
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
        return compute_pseudo_element_style_inner(doc, node, pseudo, sheet, parent, viewport, dark_mode, &[], None);
    }
    let t_pseudo = std::time::Instant::now();
    let out = compute_pseudo_element_style_inner(doc, node, pseudo, sheet, parent, viewport, dark_mode, &[], None);
    note_pseudo_cascade(pseudo, t_pseudo.elapsed().as_nanos() as u64, out.is_some());
    out
}

/// PERF-PSEUDO-CACHE — [`compute_pseudo_element_style`] backed by the
/// pass-carried [`crate::counters::PseudoCache`]. Box-tree callers pass
/// `counters.pseudo_cache()`; `walk` drops a node's entries whenever its own
/// cascade entry recomputes, which is exactly the set of nodes whose pseudo
/// match could have changed (see `PseudoCache`'s doc comment for the
/// inclusion argument). A hit still pays one `ComputedStyle` clone (~3 KB) —
/// orders of magnitude cheaper than the candidate matching it replaces.
#[allow(clippy::too_many_arguments)]
pub fn compute_pseudo_element_style_cached(
    doc: &Document,
    node: NodeId,
    pseudo: &str,
    sheet: &Stylesheet,
    parent: &ComputedStyle,
    viewport: Size,
    dark_mode: bool,
    store: &crate::counters::PseudoCache,
) -> Option<ComputedStyle> {
    let key = crate::counters::PseudoPassKey {
        rev: sheet.revision(),
        vp: [viewport.width.to_bits(), viewport.height.to_bits()],
        dark: dark_mode,
        doc: std::ptr::from_ref(doc) as usize,
    };
    if let Some(hit) = store.lookup(node, pseudo, key) {
        return hit.map(|arc| (*arc).clone());
    }
    if !matches!(doc.get(node).data, NodeData::Element { .. }) {
        return None;
    }
    // Same census as `compute_pseudo_element_style`, but the match-record
    // lookup/store below goes through the caller's `store` — box build runs
    // its children scan on rayon workers whose `CURRENT` thread-local is
    // empty, so the thread-local road alone missed every parallel probe.
    let out = if PSEUDO_STATS_ON.load(std::sync::atomic::Ordering::Relaxed) {
        let t_pseudo = std::time::Instant::now();
        let out = compute_pseudo_element_style_inner(
            doc, node, pseudo, sheet, parent, viewport, dark_mode, &[], Some(store),
        );
        note_pseudo_cascade(pseudo, t_pseudo.elapsed().as_nanos() as u64, out.is_some());
        out
    } else {
        compute_pseudo_element_style_inner(
            doc, node, pseudo, sheet, parent, viewport, dark_mode, &[], Some(store),
        )
    };
    store.store(node, pseudo, key, out.clone());
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

/// PERF-PSEUDO-PRE — the candidate-scan half of [`compute_pseudo_element_style_inner`],
/// extracted so `style::pre_match` workers can run it in parallel and the
/// sequential walk can replay the record. Pushes one [`PseudoMatch`] per
/// declaration of every matching rule; the caller sorts by
/// `(important, spec, key, decl_idx)` — the order the cascade applies them in.
/// `pseudo` is the full request (`scroll-button(name)` keeps its argument for
/// the matcher), `pseudo_base` its bare name for the per-name indexes.
#[allow(clippy::too_many_arguments)]
fn collect_pseudo_matches(
    doc: &Document,
    node: NodeId,
    pseudo: &str,
    pseudo_base: &str,
    sheet: &Stylesheet,
    viewport: Size,
    dark_mode: bool,
    part_scopes: &[crate::style::cascade::PartScope],
    out: &mut Vec<PseudoMatch>,
) {
    // BUG-1800 срез 9: one candidate buffer reused across every block below —
    // `candidates_into` clears it, so the probe pays at most one allocation
    // instead of one per block.
    let mut cand = Vec::new();
    let node_data = doc.get(node);
    let node_tag = node_data.element_name().map_or("", |q| q.local.as_str());
    let node_id = node_data.get_attr("id");
    let class_attr = node_data.get_attr("class").unwrap_or("");
    let node_attrs: &[lumen_dom::Attribute] = match &node_data.data {
        lumen_dom::NodeData::Element { attrs, .. } => attrs,
        _ => &[],
    };
    ensure_cascade_index(sheet, viewport, dark_mode);
    with_front_cascade_index(|idx| {
        // PERF-PSEUDO-BY-NAME: top-level candidates come from the index built
        // for THIS pseudo name only; a name absent from the map has no
        // top-level pseudo rules, so the loop is skipped entirely.
        if let Some((_, top_index)) = idx
            .pseudo_rules_by_name
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(pseudo_base))
        {
            for rule_idx in
                top_index.candidates_into(node_tag, node_id, class_attr.split_whitespace(), node_attrs, &mut cand)
            {
                let rule = &sheet.rules[*rule_idx];
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
                    let key = idx.order.style(*rule_idx);
                    for (decl_idx, decl) in rule.declarations.iter().enumerate() {
                        out.push(PseudoMatch {
                            important: decl.important,
                            spec,
                            key,
                            decl_idx,
                            rule: PseudoRuleRef::Top(*rule_idx as u32),
                        });
                    }
                }
            }
        }

        // `@media`/`@supports` blocks: "active" is precomputed once per
        // (sheet, viewport, dark_mode) in the shared index; per-name indexes
        // inside the block skip pseudos the block does not target.
        let blocks = idx
            .pseudo_media
            .iter()
            .filter(|b| idx.active_media[b.block])
            .map(|b| (b, &sheet.media_rules[b.block].rules, false))
            .chain(
                idx.pseudo_supports
                    .iter()
                    .filter(|b| idx.active_supports[b.block])
                    .map(|b| (b, &sheet.supports_rules[b.block].rules, true)),
            );
        for (block, rules, is_supports) in blocks {
            let Some((_, block_index)) = block
                .by_name
                .iter()
                .find(|(n, _)| n.eq_ignore_ascii_case(pseudo_base))
            else {
                continue;
            };
            for rule_idx in
                block_index.candidates_into(node_tag, node_id, class_attr.split_whitespace(), node_attrs, &mut cand)
            {
                let rule = &rules[*rule_idx];
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
                    let global_rule_idx = block.base + *rule_idx;
                    let rule_ref = if is_supports {
                        PseudoRuleRef::Supports { block: block.block as u32, rule: *rule_idx as u32 }
                    } else {
                        PseudoRuleRef::Media { block: block.block as u32, rule: *rule_idx as u32 }
                    };
                    for (decl_idx, decl) in rule.declarations.iter().enumerate() {
                        out.push(PseudoMatch {
                            important: decl.important,
                            spec,
                            key: global_rule_idx,
                            decl_idx,
                            rule: rule_ref,
                        });
                    }
                }
            }
        }
    });

    // Part scopes: the outer tree's `host::part(names)::pseudo` rules match the
    // part element. They sort after the part-less rules of their sheet (rule
    // index offset), never ahead by specificity.
    let part_pseudo_ok = |k: &PseudoElementKind| pseudo_element_matches(k, pseudo);
    for (scope_idx, scope) in part_scopes.iter().enumerate() {
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
                    out.push(PseudoMatch {
                        important: decl.important,
                        spec,
                        key: PART_RULE_BASE + i,
                        decl_idx,
                        rule: PseudoRuleRef::Part { scope: scope_idx as u32, rule: i as u32 },
                    });
                }
            }
        }
        SHADOW_HOST_SCOPE.with(|c| c.set(u32::MAX));
    }
}

/// The `&Declaration` a [`PseudoMatch`] points at. `part_scopes` must be the
/// same `part_scopes_of(doc, node)` result the match was collected under —
/// both roads compute it deterministically from the DOM.
fn resolve_pseudo_decl<'a>(
    sheet: &'a Stylesheet,
    part_scopes: &'a [crate::style::cascade::PartScope],
    m: &PseudoMatch,
) -> Option<&'a Declaration> {
    let rule = match m.rule {
        PseudoRuleRef::Top(r) => sheet.rules.get(r as usize)?,
        PseudoRuleRef::Media { block, rule } => {
            sheet.media_rules.get(block as usize)?.rules.get(rule as usize)?
        }
        PseudoRuleRef::Supports { block, rule } => {
            sheet.supports_rules.get(block as usize)?.rules.get(rule as usize)?
        }
        PseudoRuleRef::Part { scope, rule } => {
            let sc = part_scopes.get(scope as usize)?;
            let rules: &[lumen_css_parser::Rule] = match &sc.sheet {
                Some(shadow) => &shadow.rules,
                None => &sheet.rules,
            };
            return rules.get(rule as usize)?.declarations.get(m.decl_idx);
        }
    };
    rule.declarations.get(m.decl_idx)
}

/// PERF-PSEUDO-PRE — pseudo names `sheet` can target (the lowercased
/// [`crate::style::cascade_index::CascadeIndex::pseudo_subjects`] entries), for
/// `style::pre_match` workers to fan out. A name absent here can never reach
/// the matcher: the sequential road's `sheet_targets_pseudo` gate returns
/// `None` before consulting the table.
pub(in crate::style) fn pre_match_pseudo_names(
    sheet: &Stylesheet,
    viewport: Size,
    dark_mode: bool,
) -> Vec<String> {
    ensure_cascade_index(sheet, viewport, dark_mode);
    let mut names: Vec<String> =
        with_front_cascade_index(|idx| idx.pseudo_subjects.iter().map(|s| s.to_string()).collect());
    // Pseudos exempt from the `sheet_targets_pseudo` early-out reach the match
    // phase even when no rule names them (`::marker` synthesizes from
    // `list-style-type`, `q::before`/`::after` from the UA quote, scroll
    // markers likewise). Workers must record their — usually empty — matches
    // or every probe of them falls back to the sequential candidate scan.
    for extra in ["marker", "scroll-marker-group", "before", "after"] {
        if !names.iter().any(|n| n.eq_ignore_ascii_case(extra)) {
            names.push(extra.to_string());
        }
    }
    names
}

/// PERF-PSEUDO-PRE — `node`'s matched pseudo declarations for `pseudo`, as an
/// owned, cascade-sorted record for `style::pre_match`. `None` when the sheet
/// cannot target the name at all (the same `sheet_targets_pseudo` gate the
/// sequential road applies before matching).
#[allow(clippy::too_many_arguments)]
pub(in crate::style) fn match_pseudo_owned(
    doc: &Document,
    node: NodeId,
    pseudo: &str,
    sheet: &Stylesheet,
    viewport: Size,
    dark_mode: bool,
) -> Option<std::sync::Arc<[PseudoMatch]>> {
    let pseudo_base = split_pseudo_request(pseudo).0;
    let part_scopes = part_scopes_of(doc, node);
    if !sheet_targets_pseudo(sheet, viewport, dark_mode, pseudo_base) {
        // No rule of this sheet can match the pseudo — but `::part()` scopes
        // of outer trees still can, so only a node without part scopes has a
        // provably empty record. Returning it lets `pre_match` workers store
        // "matched nothing" instead of leaving every exempt probe
        // (`::marker`, `q::before`, …) to rescan the candidate index.
        if part_scopes.is_empty() {
            return Some(empty_pseudo_matches());
        }
    }
    let mut out = Vec::new();
    collect_pseudo_matches(doc, node, pseudo, pseudo_base, sheet, viewport, dark_mode, &part_scopes, &mut out);
    out.sort_by_key(|m| (m.important, m.spec, m.key, m.decl_idx));
    Some(if out.is_empty() {
        empty_pseudo_matches()
    } else {
        std::sync::Arc::from(out)
    })
}

/// PERF-PSEUDO-STORE: the overwhelmingly common record is "matched nothing" —
/// `pre_match` workers produce one per (node, pseudo-name) pair, tens of
/// thousands per flush. A shared allocation turns each into a refcount bump
/// instead of a fresh `Arc` box.
fn empty_pseudo_matches() -> std::sync::Arc<[PseudoMatch]> {
    static EMPTY: std::sync::LazyLock<std::sync::Arc<[PseudoMatch]>> =
        std::sync::LazyLock::new(|| std::sync::Arc::from(Vec::new()));
    std::sync::Arc::clone(&EMPTY)
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
    ua_decls: &[Declaration],
    match_cache: Option<&crate::counters::PseudoCache>,
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
        && ua_decls.is_empty()
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
    // PERF-PSEUDO-PRE: a cascade pass may already have matched this node's
    // pseudo rules in parallel (`style::pre_match::pseudo_lookup`) — the record
    // is the same `PseudoMatch` list the sequential road below builds, only the
    // candidate scan is gone. Callers that pass `ua_decls` (the view-transition
    // tree) always take the sequential road: their declarations are per-call.
    let pseudo_base = split_pseudo_request(pseudo).0;
    // Functional requests (`::view-transition-old(name)`, `::scroll-button(dir)`,
    // `::highlight(name)`) match on their argument, which the bare-name record
    // does not carry — they always take the sequential road.
    // `match_cache` is the caller-passed cache (rayon-safe); callers without
    // one still see the pass-installed thread-local.
    let current_cache;
    let match_cache = match match_cache {
        Some(c) => Some(c),
        None => {
            current_cache = crate::counters::PseudoCache::current();
            current_cache.as_deref()
        }
    };
    let pre = (ua_decls.is_empty() && split_pseudo_request(pseudo).1.is_none())
        .then(|| {
            super::pre_match::pseudo_lookup(node, pseudo_base, sheet, viewport, dark_mode).or_else(|| {
                // Box build runs after `pre_match`'s guard dropped — the same
                // records were bulk-loaded into the pass's `PseudoCache`, which
                // `box_tree::entry` installs for the duration of the build.
                let key = crate::counters::pseudo_pass_key(doc, sheet, viewport, dark_mode);
                match_cache?.lookup_match(node, pseudo_base, key)
            })
        })
        .flatten();
    let mut matched_vec: Vec<PseudoMatch> = Vec::new();
    let matched: &[PseudoMatch] = if let Some(rec) = &pre {
        rec
    } else {
        collect_pseudo_matches(
            doc, node, pseudo, pseudo_base, sheet, viewport, dark_mode, &part_scopes, &mut matched_vec,
        );
        matched_vec.sort_by_key(|m| (m.important, m.spec, m.key, m.decl_idx));
        // A sequential answer is still a pure function of the pass inputs —
        // keep it so the next probe of this (node, pseudo) skips the scan.
        if let Some(cache) = match_cache {
            let key = crate::counters::pseudo_pass_key(doc, sheet, viewport, dark_mode);
            cache.store_match(node, pseudo_base, key, std::sync::Arc::from(matched_vec.clone()));
        }
        &matched_vec
    };

    if matched.is_empty() && ua_decls.is_empty() {
        // CSS Lists L3 §2.1: ::marker always generates a marker box from list-style-type
        // without any explicit CSS rule. Other pseudo-elements require a matching declaration.
        if pseudo.eq_ignore_ascii_case("marker") {
            let _prof_init = lumen_core::profile::scope_detail("ps_init");
            return Some(marker_base_style(parent));
        }
        if let Some(item) = ua_quote {
            let mut style = pseudo_inherited_style(parent);
            style.content = Content::Items(std::sync::Arc::new(vec![item]));
            return Some(style);
        }
        // CSS Overflow L5 §scroll-marker-group: an originating element whose
        // `scroll-marker-group` is not `none` gets the group box even with no author rule.
        if is_scroll_group {
            let mut style = pseudo_inherited_style(parent);
            style.display = Display::Block;
            // Группа — сосед originating-элемента: flex/grid-item, когда flex/grid
            // и его родитель (CSS Flexbox L1 §4.3 — `z-index` на static-item).
            style.is_flex_grid_item = parent.is_flex_grid_item;
            return Some(style);
        }
        return None;
    }
    drop(prof_match);
    let mut style = {
        let _prof_init = lumen_core::profile::scope_detail("ps_init");
        let mut style = if pseudo.eq_ignore_ascii_case("marker") { marker_base_style(parent) } else { pseudo_inherited_style(parent) };
        if let Some(item) = ua_quote {
            style.content = Content::Items(std::sync::Arc::new(vec![item]));
        }
        if is_scroll_group {
            style.display = Display::Block;
            style.is_flex_grid_item = parent.is_flex_grid_item;
        }
        if split_pseudo_request(pseudo).0.eq_ignore_ascii_case("scroll-button") {
            // `::scroll-button()` — сосед originating-элемента в его родителе:
            // flex/grid-item при flex/grid-родителе (см. `is_scroll_group`).
            style.is_flex_grid_item = parent.is_flex_grid_item;
            super::ua::apply_ua_scroll_button(&mut style, dark_mode);
        }
        style
    };
    // CSS Basic UI L4 §appearance-disabling-properties / HTML LS §15.5:
    // `::scroll-button()` is a widget in its own right — an author
    // `background`/`border`/`padding` declaration (or `appearance: none`)
    // drops the UA box styling exactly like on `<button>`; the origin
    // element's tag is irrelevant (WPT `scroll-buttons-*` refs compare it
    // against a real `<button>`, whose UA border disappears the same way).
    // Runs before the declaration loop below so author values land on the
    // stripped basis, mirroring `cascade`'s `disabled_by_author` pass.
    if split_pseudo_request(pseudo).0.eq_ignore_ascii_case("scroll-button") {
        let mut appearance_none = false;
        let mut appearance_auto = true;
        for m in matched {
            let Some(decl) = resolve_pseudo_decl(sheet, &part_scopes, m) else {
                continue;
            };
            if matches!(decl.property.as_str(), "appearance" | "-webkit-appearance") {
                appearance_none = decl.value.trim().eq_ignore_ascii_case("none");
                appearance_auto = decl.value.trim().eq_ignore_ascii_case("auto");
            }
        }
        let disabled_by_author = appearance_auto
            && matched.iter().any(|m| {
                resolve_pseudo_decl(sheet, &part_scopes, m)
                    .is_some_and(|d| super::ua::is_appearance_disabling_property(&d.property))
            });
        if appearance_none || disabled_by_author {
            for w in [
                &mut style.border_top_width,
                &mut style.border_right_width,
                &mut style.border_bottom_width,
                &mut style.border_left_width,
            ] {
                *w = 0.0;
            }
            style.border_top_style = crate::style::BorderStyle::None;
            style.border_right_style = crate::style::BorderStyle::None;
            style.border_bottom_style = crate::style::BorderStyle::None;
            style.border_left_style = crate::style::BorderStyle::None;
            style.padding_top = crate::style::Length::Px(0.0);
            style.padding_right = crate::style::Length::Px(0.0);
            style.padding_bottom = crate::style::Length::Px(0.0);
            style.padding_left = crate::style::Length::Px(0.0);
            style.background_color = Some(crate::style::CssColor::Rgba(crate::style::Color {
                r: 0,
                g: 0,
                b: 0,
                a: 0,
            }));
            if disabled_by_author {
                style.appearance = crate::Appearance::None;
            }
        }
    }
    let _prof_apply = lumen_core::profile::scope_detail("ps_apply");

    // `matched` arrives sorted — `collect_pseudo_matches` callers sort by the
    // same (important, spec, key, decl_idx) key the old in-place sort used.

    let parent_fs = parent.font_size;
    let is_quirks = doc.mode() == DocumentMode::Quirks;
    for m in matched {
        // Pseudo-element style: the basis is irrelevant here — `zoom` is folded
        // into the originating element's style, which this one inherits from.
        if let Some(decl) = resolve_pseudo_decl(sheet, &part_scopes, m) {
            let _ = apply_font_size(&mut style, decl, parent_fs, parent_fs, viewport, is_quirks);
        }
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
    // `ua_decls` are the UA-origin declarations (the `::view-transition-*` tree's UA sheet): they
    // come first, so every author declaration overrides them whatever its specificity.
    for decl in ua_decls
        .iter()
        .chain(matched.iter().filter_map(|m| resolve_pseudo_decl(sheet, &part_scopes, m)))
    {
        if (is_marker && !marker_property_applies(&decl.property))
            || (is_first_line && !first_line_property_applies(&decl.property))
        {
            continue;
        }
        let attr_buf;
        let effective_decl: &Declaration = if decl.value.contains("attr(") {
            let Some(v) = expand_attr_val(&decl.value, doc, node) else { continue };
            attr_buf = Declaration { flags: lumen_css_parser::decl_content_flags(&v), property: decl.property.clone(), value: v, important: decl.important };
            &attr_buf
        } else {
            decl
        };
        apply_declaration(&mut style, effective_decl, em_basis, viewport, parent_weight, parent, parent, is_quirks, dark_mode);
    }

    // Same as the end of `compute_style`: a `none`/`hidden` border side computes to width 0.
    super::cascade::zero_unstyled_border_widths(&mut style);

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
        // by the UA, not by `content:` — no `content` required. Its UA sheet is written with
        // logical longhands (`inset-block-start`, `inline-size`), which read back as their
        // physical twins.
        if split_pseudo_request(pseudo).0.starts_with("view-transition") {
            crate::style::logical::resolve_logical_properties(&mut style);
        }
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

/// The UA style sheet of the `::view-transition` pseudo-tree (CSS View Transitions L1 §6.1–6.5,
/// "UA style sheet for view transition pseudo-elements") for one part and capture name, as
/// declarations. `part` is `None` for the `::view-transition` root of the tree. `copied` are the
/// properties the group takes from the captured element once the transition is activated
/// (`mix-blend-mode`, `text-orientation`, `color-scheme`, …) — they join the group's declarations.
fn view_transition_ua_decls(
    part: Option<ViewTransitionPart>,
    name: &str,
    copied: &[(String, String)],
    parent_map: &std::collections::HashMap<String, String>,
) -> Vec<Declaration> {
    // The UA sheet's `animation-*: inherit`: the computed style has no CSS-wide keyword for the
    // animation lists, so the parent's computed value is written out.
    let inherited = |prop: &str| parent_map.get(prop).map_or("inherit", String::as_str).to_string();
    let decl = |property: &str, value: &str| Declaration {
        flags: lumen_css_parser::decl_content_flags(value),
        property: property.to_string(),
        value: value.to_string(),
        important: false,
    };
    let mut out = Vec::new();
    match part {
        None => {
            // `absolute`, not the older draft's `fixed`: the tree's containing block is the
            // snapshot viewport (WPT `pseudo-get-computed-style`).
            out.push(decl("position", "absolute"));
            out.push(decl("inset", "0"));
        }
        Some(ViewTransitionPart::Group) => {
            out.push(decl("position", "absolute"));
            out.push(decl("top", "0"));
            out.push(decl("left", "0"));
            out.push(decl("animation-duration", "0.25s"));
            out.push(decl("animation-fill-mode", "both"));
            out.push(decl("animation-name", &format!("-ua-view-transition-group-anim-{name}")));
            out.extend(copied.iter().map(|(p, v)| decl(p, v)));
        }
        Some(ViewTransitionPart::ImagePair) => {
            out.push(decl("position", "absolute"));
            out.push(decl("inset", "0"));
            out.push(decl("animation-duration", &inherited("animation-duration")));
            out.push(decl("animation-fill-mode", &inherited("animation-fill-mode")));
            out.push(decl("isolation", "isolate"));
        }
        Some(part @ (ViewTransitionPart::Old | ViewTransitionPart::New)) => {
            out.push(decl("position", "absolute"));
            out.push(decl("inset-block-start", "0"));
            out.push(decl("inline-size", "100%"));
            out.push(decl("block-size", "auto"));
            out.push(decl("animation-duration", &inherited("animation-duration")));
            out.push(decl("animation-fill-mode", &inherited("animation-fill-mode")));
            out.push(decl("mix-blend-mode", "plus-lighter"));
            let fade = if part == ViewTransitionPart::Old { "out" } else { "in" };
            out.push(decl("animation-name", &format!("-ua-view-transition-fade-{fade}")));
        }
    }
    out
}

/// The full computed style of one node of the `::view-transition` pseudo-tree — the UA style
/// sheet of [`view_transition_ua_decls`] under the author rules, with the parent chain
/// `::view-transition` → group → image-pair → old/new, so `inherit` (the UA sheet's
/// `animation-duration: inherit` on the image pair and the captures) reads the parent's value.
/// `part` is `None` for `::view-transition` itself; `copied` as for
/// [`view_transition_ua_decls`] (used for the group only).
///
/// Returns the style whether or not an author rule targets the node: the caller decides whether
/// the node exists (a transition is active and `name` is one of its captures).
pub fn compute_view_transition_tree_style(
    doc: &Document,
    sheet: &Stylesheet,
    part: Option<ViewTransitionPart>,
    name: &str,
    copied: &[(String, String)],
    viewport: Size,
    dark_mode: bool,
) -> Option<ComputedStyle> {
    let root = doc.document_element()?;
    // Chain of (part, request) from the tree root down to the asked node.
    let chain: &[Option<ViewTransitionPart>] = match part {
        None => &[None],
        Some(ViewTransitionPart::Group) => &[None, Some(ViewTransitionPart::Group)],
        Some(ViewTransitionPart::ImagePair) => {
            &[None, Some(ViewTransitionPart::Group), Some(ViewTransitionPart::ImagePair)]
        }
        Some(p @ (ViewTransitionPart::Old | ViewTransitionPart::New)) => {
            &[None, Some(ViewTransitionPart::Group), Some(ViewTransitionPart::ImagePair), Some(p)]
        }
    };
    let mut parent = ComputedStyle::root();
    for &step in chain {
        let request = step.map_or_else(|| "view-transition".to_string(), |p| p.request(name));
        let parent_map = crate::computed_style_to_map(&parent);
        let own_copied = if step == Some(ViewTransitionPart::Group) { copied } else { &[] };
        let decls = view_transition_ua_decls(step, name, own_copied, &parent_map);
        parent = compute_pseudo_element_style_inner(doc, root, &request, sheet, &parent, viewport, dark_mode, &decls, None)?;
    }
    Some(parent)
}
