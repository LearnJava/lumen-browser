//! Главный проход каскада: `compute_style` — построение `ComputedStyle` узла
//! из UA-таблицы, презентационных атрибутов, author-правил и инлайнового
//! `style`, — плюс CSS Viewport L1 §5 `zoom` и счётчик полных проходов
//! (BUG-341 S18).
//!
//! Перенесено батчем SPLIT-ST14 из `crates/engine/layout/src/style.rs`
//! (анкер `static COMPUTE_STYLE_CALLS`) без правок тел.

use crate::style::calc::{looks_like_function_call, parse_math_function_value};
use std::collections::HashMap;

use lumen_core::geom::Size;
use lumen_css_parser::{
    parse_inline_style, Declaration, FunctionRule, MixinRule, PropertyRule, Specificity, Stylesheet,
    MIXIN_APPLY_MARKER,
};
use lumen_dom::{Document, DocumentMode, NodeData, NodeId};

use crate::font_palette::resolve_font_palette_overrides;
use crate::style::presentational::is_svg_presentational_element;
use crate::style::share_safety::selector_is_share_safe;
use crate::style::{
    apply_align_presentational_hint, apply_background_image_presentational_hint,
    apply_bgcolor_presentational_hint, apply_bordercolor_presentational_hint,
    apply_cellspacing_presentational_hint, apply_declaration, apply_dir_presentational_hint,
    apply_font_element_presentational_hints, apply_font_size, apply_forced_colors_mode,
    apply_image_presentational_hints, apply_property_initial_values, apply_quirks_html_height,
    apply_quirks_line_height, apply_quirks_table_reset, apply_svg_presentational_hints,
    apply_table_cell_width_hint, apply_text_color_presentational_hint, apply_ua_body_margin,
    apply_ua_dialog_display, apply_ua_fieldset_style, apply_ua_form_controls, apply_ua_form_controls_field_sizing_clear,
    apply_ua_heading_style, apply_ua_hidden, apply_ua_hr_style, apply_ua_inert, apply_ua_slot, apply_ua_table_cell_padding,
    apply_ua_text_decoration, apply_webkit_scrollbar_pseudos, coerce_overflow_axes,
    complex_has_host, default_display, ensure_cascade_index, expand_attr_val,
    expand_custom_functions_scoped, expand_mixin_apply, expand_vars, forced_colors_active, matches_complex,
    matches_slotted_complex, node_in_scope, resolve_logical_properties, resolve_overflow_logical_properties,
    resolve_overscroll_behavior_logical_properties, resolve_system_colors_in_style,
    strip_ua_appearance_box_styling, ua_font_family,
    ua_font_size_factor, ua_font_style, ua_font_weight, ua_link_color, ua_vertical_align,
    parse_css_wide_keyword, ua_white_space, validate_against_syntax, with_front_cascade_index,
    ComputedStyle, CssContinue, CssWideKeyword, Display, FlexDirection, WebkitBoxOrient,
    FieldSizing, FontPalette, FontSizeBasis, FontWeight, Length, LengthOrAuto, Overflow,
    SHADOW_HOST_SCOPE, SHADOW_SHEETS,
};

/// BUG-341 S18 — process-wide tally of full [`compute_style`] runs.
///
/// The cascade stage's own [`crate::counters::CascadeStats`] counts only the
/// calls `counters::walk` makes. It cannot see the ones the box-build stage
/// makes behind its back: `is_inline_content` / `is_inline_block` probe every
/// child of every rebuilt container with a fresh `compute_style` instead of the
/// `CounterMap` cache `build_box` itself uses, and non-element nodes have no
/// cache entry at all. Process-wide (an atomic, not a thread-local) because
/// `build_box` fans out over rayon workers — the S15 trap.
static COMPUTE_STYLE_CALLS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Returns the number of [`compute_style`] runs since the last drain, and
/// resets the tally (see [`COMPUTE_STYLE_CALLS`]).
pub fn take_compute_style_calls() -> u64 {
    COMPUTE_STYLE_CALLS.swap(0, std::sync::atomic::Ordering::Relaxed)
}

/// Bumps the [`COMPUTE_STYLE_CALLS`] tally.
fn note_compute_style() {
    COMPUTE_STYLE_CALLS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
}

/// CSS Viewport L1 §5 — parse the specified value of `zoom`.
///
/// Accepted: a non-negative `<number>` (`0.8`, `.8`, `1`), a `<percentage>`
/// (`80%`), and the keywords `normal` / `reset`, both of which mean "no scaling
/// of my own" and so yield `1.0`. (`reset`'s real WebKit semantics — ignore the
/// ancestors' zoom rather than merely contributing 1.0 — are not modelled;
/// nothing in the wild depends on it and it would need a separate flag.)
///
/// Returns `None` when the value does not parse, in which case the caller must
/// leave the previous value alone — an invalid declaration is ignored, per
/// CSS Syntax, not treated as `1.0`.
pub(in crate::style) fn parse_zoom(value: &str, em_basis: f32) -> Option<f32> {
    let v = value.trim();
    if v.eq_ignore_ascii_case("normal") || v.eq_ignore_ascii_case("reset") {
        return Some(1.0);
    }
    let factor = if let Some(pct) = v.strip_suffix('%') {
        match pct.trim().parse::<f32>() {
            Ok(n) => n / 100.0,
            Err(_) => parse_zoom_math(v, em_basis)?,
        }
    } else if let Ok(n) = v.parse::<f32>() {
        n
    } else {
        parse_zoom_math(v, em_basis)?
    };
    // A negative or non-finite zoom is invalid; a zero one would collapse the
    // subtree to nothing, which no page means and which would divide by zero
    // when un-zooming. Both are rejected so the declaration is simply dropped.
    if !factor.is_finite() || factor <= 0.0 {
        return None;
    }
    Some(factor)
}

/// `zoom: calc(...)` / `sign(...)` etc. (CSS Values L4 §10): the math function
/// is evaluated with `%` taken against 1 (so `2%` → 0.02) and `em` against the
/// inherited font size, since `zoom` is resolved before this element's font-size.
fn parse_zoom_math(v: &str, em_basis: f32) -> Option<f32> {
    if !looks_like_function_call(v) {
        return None;
    }
    let Length::Calc(node) = parse_math_function_value(v)? else {
        return None;
    };
    node.resolve(em_basis, Some(1.0), Size { width: 0.0, height: 0.0 })
}

/// Scale one already-computed absolute length by `z`. Only `Px` and the viewport
/// units are touched: every other unit resolves later against a basis
/// (`font_size`, the containing block) that is itself already zoomed, so scaling
/// here too would apply the factor twice.
/// CSS Scoping L1 §3.5 (BUG-519): the tree-scoped `@function` lookup chain
/// for a declaration that came from `sheet`, the stylesheet of the shadow
/// tree hosted by `owner_host`. Innermost first: `sheet`'s own functions,
/// then the functions of each shadow tree that encloses `owner_host`, out to
/// (but excluding) the document — the caller appends the document's own
/// `function_rules` last. `None` when no tree in the chain declares any
/// function, so the caller keeps the plain document-only lookup.
fn shadow_function_chain(doc: &Document, sheet: &Stylesheet, owner_host: NodeId) -> Option<Vec<Vec<FunctionRule>>> {
    let mut chain = vec![sheet.function_rules.clone()];
    let mut cur = doc.enclosing_shadow_host(owner_host);
    while let Some(host) = cur {
        chain.push(SHADOW_SHEETS.with(|c| c.borrow().get(&host).map(|s| s.function_rules.clone()).unwrap_or_default()));
        cur = doc.enclosing_shadow_host(host);
    }
    chain.iter().any(|f| !f.is_empty()).then_some(chain)
}

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
fn apply_zoom_to_lengths(style: &mut ComputedStyle, z: f32, rem_k: f32, root_k: f32) {
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
}

/// Computes the `ComputedStyle` for `node` by running the CSS cascade.
///
/// `dark_mode` is forwarded to `@media (prefers-color-scheme: dark)` matching.
pub fn compute_style(
    doc: &Document,
    node: NodeId,
    sheet: &Stylesheet,
    inherited: &ComputedStyle,
    viewport: Size,
    dark_mode: bool,
) -> ComputedStyle {
    compute_style_shareable(doc, node, sheet, inherited, viewport, dark_mode).0
}

/// THREAD-4 срез 2 — [`compute_style`] plus whether the result is safe to
/// memoise in [`crate::style::share_cache::ShareCache`] across *different*
/// nodes that share the same structural key (tag + full attribute set +
/// identical `inherited` allocation).
///
/// `shareable` starts `true` and is only ever narrowed to `false`: any
/// stylesheet feature whose match result can depend on something the
/// structural key does not capture disqualifies the whole call. Concretely:
/// `@scope` (position-in-tree), any Shadow DOM in the document (`:host`/
/// `::slotted` scoping is keyed on thread-local host state, not on the key),
/// and any candidate rule whose selector has a sibling combinator
/// (`+`/`~` — matches against siblings, which nothing in the key captures at
/// any level) or a pseudo-class/attribute-selector part (`:nth-child`,
/// `:hover`, `[data-x]`, …- structural or dynamic-state dependence the key
/// does not model). `Descendant`/`Child` combinators do *not* disqualify —
/// see [`selector_is_share_safe`]'s doc comment (BUG-1112) for why the
/// `inherited_ptr` half of the key already proves ancestor-chain identity
/// whenever it collides. Kept to SVG presentational elements
/// (`is_svg_presentational_element`) for this slice: the HTML-side
/// presentational-hint/quirks passes below
/// (`apply_bgcolor_presentational_hint`, table/form/quirks helpers) are not
/// audited for ancestor independence, whereas `apply_svg_presentational_hints`
/// is a pure function of the node's own attributes plus `inherited`/`viewport`
/// (already covered by the key) — see срез 1's profiling (`ROADMAP.md`
/// THREAD-4): the measured cost is exactly repeated SVG icon markup
/// (GitHub Octicons), so this scope covers the case that motivated the slice
/// without auditing the rest of the cascade's ancestor-dependence.
pub(crate) fn compute_style_shareable(
    doc: &Document,
    node: NodeId,
    sheet: &Stylesheet,
    inherited: &ComputedStyle,
    viewport: Size,
    dark_mode: bool,
) -> (ComputedStyle, bool) {
    // BUG-341 S10: permanent per-phase instrumentation. Same-named sibling
    // scopes are merged by `lumen_core::profile`, so a `LUMEN_PROFILE_TREE=1`
    // run prints one aggregated line per phase with a `×N` call count instead
    // of one line per node. Costs a cached bool check per phase when disabled.
    let _prof = lumen_core::profile::scope_detail("compute_style");
    note_compute_style();
    // CSS Color L5 §5.3 — `color(--name …)` is parsed deep inside
    // `apply_declaration`, which never sees the sheet: bind its
    // `@color-profile` rules to this thread first.
    crate::style::parse::color::sync_color_profiles(sheet);
    let mut shareable = sheet.scope_rules.is_empty();
    let prof_init = lumen_core::profile::scope_detail("cs_init");
    let mut style = ComputedStyle::inheriting(inherited);
    style.display = default_display(doc, node);
    // `inherited` is the parent's computed style: a flex/grid parent makes this
    // box an item, and a `z-index` on a static item must create a stacking
    // context (CSS Flexbox L1 §4.3, Grid L1 §6.4).
    style.is_flex_grid_item = matches!(
        inherited.display,
        Display::Flex | Display::InlineFlex | Display::Grid | Display::InlineGrid
    );
    if let Some(ws) = ua_white_space(doc, node) {
        style.white_space = ws;
        style.white_space_collapse = ws.collapse_component();
    }

    // CSS Properties and Values L1 §1.1 — registry зарегистрированных
    // custom-properties. Карта строится локально для каждого узла:
    // на типичной странице 0..5 @property-правил, накладные расходы мизерны
    // в сравнении со стоимостью каскада. При повторе имени (см. spec —
    // last wins) `insert` корректно сохраняет последнее объявление.
    let registry: HashMap<&str, &PropertyRule> = sheet
        .properties
        .iter()
        .map(|p| (p.name.as_str(), p))
        .collect();

    // Откатываем у себя унаследованные значения тех зарегистрированных
    // custom-properties, у которых `inherits: false` — для них потомок
    // должен видеть либо локальную декларацию, либо initial-value, а не
    // родительское значение.
    //
    // BUG-341 S9: `retain` needs a `make_mut`, which copies the inherited map —
    // so first check whether any key would actually be dropped. Pages that
    // register no `inherits: false` property (or declare none of the ones they
    // do register) keep sharing the parent's allocation.
    //
    // BUG-683 срез 8: a key whose inherited value already *is* its valid
    // initial-value is not dropped — `apply_property_initial_values` below
    // would put the identical string straight back, so dropping it only buys
    // a full copy of the map. Once the root has the initial value, every
    // descendant inherits it, so without this one `@property … { inherits:
    // false; initial-value: … }` (Primer's `--dialog-scrollgutter` on
    // github.com) copied the ~2000-entry map on every single node.
    let resets_inherited = |key: &str, value: &str| {
        registry.get(key).is_some_and(|p| {
            !p.inherits
                && !p.initial_value.as_deref().is_some_and(|iv| {
                    iv == value && validate_against_syntax(iv, &p.syntax)
                })
        })
    };
    if !registry.is_empty()
        && style
            .custom_props
            .iter()
            .any(|(key, value)| resets_inherited(key, value))
    {
        style
            .custom_props
            .make_mut()
            .retain(|key, value| !resets_inherited(key, value));
    }

    if !matches!(doc.get(node).data, NodeData::Element { .. }) {
        // Для не-элементов (Document, Text внутри anonymous-wrapping) тоже
        // применяем initial-value: var(--registered) в наследуемом стиле
        // должен резолвиться через initial-value, если декларации нет.
        apply_property_initial_values(&mut style.custom_props, &registry);
        return (style, false);
    }
    drop(prof_init);
    let prof_ua = lumen_core::profile::scope_detail("cs_ua_hints");

    // UA stylesheet: семантические элементы получают italic / bold по
    // умолчанию, CSS-декларации ниже могут это переопределить.
    if let Some(fs) = ua_font_style(doc, node) {
        style.font_style = fs;
    }
    if let Some(fw) = ua_font_weight(doc, node) {
        style.font_weight = fw;
    }
    // UA stylesheet: <pre>/<code>/<kbd>/<samp>/<tt> → font-family: monospace.
    if let Some(fam) = ua_font_family(doc, node) {
        style.font_family = fam;
    }
    // UA stylesheet: text-decoration для <del>/<s> (line-through),
    // <ins>/<u>/<a href> (underline). HTML5 §15.3.7.
    apply_ua_text_decoration(doc, node, &mut style);
    // UA stylesheet: <a href> → color: #0000ee. HTML5 §15.3.3.
    if let Some(c) = ua_link_color(doc, node) {
        style.color = c;
    }
    // UA stylesheet: <small>/<sub>/<sup> → font-size: 0.83× parent.
    // HTML5 §15.3.3. Author font-size перекроет через pre-pass.
    if let Some(factor) = ua_font_size_factor(doc, node) {
        style.font_size = inherited.font_size * factor;
    }
    // UA stylesheet: <sub>/<sup> и табличные элементы → vertical-align. HTML5 §15.3.3, §15.3.8.
    if let Some(va) = ua_vertical_align(doc, node, inherited.vertical_align) {
        style.vertical_align = va;
    }
    // UA stylesheet: <h1>–<h6> → font-size + vertical margins. HTML Rendering §15.3.3.
    // Set font-size here (before the author font-size pre-pass) so author CSS overrides it.
    apply_ua_heading_style(doc, node, inherited, &mut style);
    apply_ua_hr_style(doc, node, &mut style);
    // UA stylesheet: <fieldset> / <legend>. HTML Rendering §15.3.13. Author CSS перекроет.
    apply_ua_fieldset_style(doc, node, &mut style);
    // UA stylesheet: <body> → margin: 8px. HTML Rendering §14.3.3. Author CSS перекроет.
    apply_ua_body_margin(doc, node, &mut style);
    // UA stylesheet: form controls — display, intrinsic dimensions, border,
    // background, and foreground color. HTML5 §15.5. Author CSS поверх перекроет.
    //
    // CSS Color Adjustment L1 §2.3: тема UA-виджета определяется «used color
    // scheme» элемента, а не сырым предпочтением ОС. `color-scheme` наследуется,
    // поэтому на этапе UA-фазы (до author-каскада) берём inherited-значение —
    // оно покрывает типовой паттерн `:root { color-scheme: dark }`, спускающийся
    // к контролам. Так `color-scheme: light` форсирует светлый виджет даже в
    // OS-dark, а `dark` — тёмный в OS-light.
    //
    // CSS: system-color — P4 wires `system_color()` into the color cascade
    // (a `CssColor::System(name)` variant resolved at used-value time against
    // the element's used color scheme) for `Canvas`/`CanvasText`/`ButtonFace`/…
    // keyword support. The resolution table already lives in `system_color()`.
    let widget_dark = inherited.color_scheme.used_dark(dark_mode);
    apply_ua_form_controls(doc, node, &mut style, widget_dark);
    // UA stylesheet: <dialog> without `open` → display:none. HTML5 §15.3.9.
    apply_ua_dialog_display(doc, node, &mut style);
    // UA stylesheet: <td>/<th> → padding: 1px (HTML Rendering §15.3.8); the
    // ancestor <table cellpadding=N> overrides it. Author `padding` wins.
    apply_ua_table_cell_padding(doc, node, &mut style);
    // UA stylesheet (HTML Rendering §15.4.2): `[inert] { pointer-events: none; }`.
    // Applied during the pre-cascade UA phase so author `pointer-events` wins.
    apply_ua_inert(doc, node, &mut style);
    // UA stylesheet (HTML LS §3.2.6.2 / Rendering §hiddenCSS): `hidden` →
    // display:none; `hidden="until-found"` → content-visibility:hidden.
    // Author `display`/`content-visibility` declarations win (UA origin).
    apply_ua_hidden(doc, node, &mut style);
    // UA stylesheet (HTML LS §15.5.4): `<details>`' content slot is a block,
    // `content-visibility: hidden` while closed (GAP-UASHADOWSLOT).
    apply_ua_slot(doc, node, &mut style);

    // CSS Quirks Mode — Quirks-only UA-rule для `<table>`: сбрасывает
    // font / color / text-align / white-space к initial-values, чтобы
    // legacy table-layout страницы (где CSS на `<body>` задавал шрифт /
    // цвет) рендерились с дефолтным шрифтом таблицы, как в IE/Netscape.
    // В Standards / LimitedQuirks не применяется.
    apply_quirks_table_reset(doc, node, &mut style);
    // CSS Quirks Mode §3.2: replaced-элементы получают line-height: 1 как UA-правило.
    apply_quirks_line_height(doc, node, &mut style);
    // CSS Quirks Mode §3.5: <html> получает height: 100vh как UA-правило,
    // чтобы body { height: 100% } резолвилось против viewport.
    apply_quirks_html_height(doc, node, &mut style);

    // HTML presentational hints (HTML5 §10): для `<img>` атрибуты
    // `width`/`height` задают начальные значения соответствующих CSS-свойств.
    // Применяются ДО CSS-каскада, поэтому любое author-CSS правило
    // перекроет атрибут даже с specificity (0,0,1). Парсятся как unitless
    // целые пиксели — это HTML5 правило для `<img>`, единицы и проценты
    // в этих атрибутах игнорируются.
    apply_image_presentational_hints(doc, node, &mut style);

    // HTML5 §15 «Rendering»: `bgcolor` на `<body>` / `<table>` / `<thead>` /
    // `<tbody>` / `<tfoot>` / `<tr>` / `<td>` / `<th>` мапается на
    // `background-color` (presentational hint). Парсится по HTML5 §2.4.6
    // «rules for parsing a legacy color value» — более лояльный алгоритм,
    // чем CSS quirks hashless hex: принимает named colors, `#rgb` / `#rrggbb`,
    // hashless hex произвольной длины и любую строку, в которой можно
    // найти хотя бы какие-то hex-digits после padding-procedure.
    apply_bgcolor_presentational_hint(doc, node, &mut style);

    // HTML LS §15.3.8 «Tables»: `background`/`bordercolor`/`cellspacing`
    // presentational hints (BUG-603 point 2) — siblings of `bgcolor` above,
    // narrower in scope (table-tree elements only, `cellspacing` table-only).
    apply_background_image_presentational_hint(doc, node, &mut style);
    apply_bordercolor_presentational_hint(doc, node, &mut style);
    apply_cellspacing_presentational_hint(doc, node, &mut style);

    // HTML5 §15.3.6 «The page»: `text` атрибут на `<body>` и `<font color>`
    // на любом элементе мапаются на CSS `color` (presentational hint).
    // Парсятся тем же legacy-парсером, что и `bgcolor`. Author CSS поверх —
    // выигрывает. `<body link/vlink/alink>` отложены: `:link` единственный
    // матчится в Phase 0, `:visited`/`:active` без runtime — no-op.
    apply_text_color_presentational_hint(doc, node, &mut style);

    // HTML5 §15.3.2: `<font size>` → font-size; `<font face>` → font-family.
    apply_font_element_presentational_hints(doc, node, &mut style);

    // HTML5 §15.3.3: `align` на блочных элементах → text-align.
    apply_align_presentational_hint(doc, node, &mut style);

    // HTML LS §15.3.6: `dir` → `direction` + `unicode-bidi` (BUG-1321).
    apply_dir_presentational_hint(doc, node, &mut style);

    // CSS Quirks Mode §4.1 + HTML5 §14.3.9: `width`/`height` attr на
    // `<td>`/`<th>`/`<table>`. В quirks-mode width ячейки → min-width.
    apply_table_cell_width_hint(doc, node, &mut style);

    // CSS Cascade L4 §6.4.3 — inline style: парсим HTML-атрибут `style=""`
    // и кладём его декларации в отдельный буфер. Они подключаются к каскаду
    // через дополнительный sort-bit `is_inline` (ниже): внутри одного origin
    // (нормального или !important) inline всегда побеждает любой селектор —
    // это «Element-Attached Styles» тир в Cascade L4 §8.1, идущий после
    // Layer/Specificity/Order, но до Importance-инверсии.
    drop(prof_ua);
    let prof_match = lumen_core::profile::scope_detail("cs_match");
    // GAP-CSPENF срез 23: a `style-src-attr`-blocked `style=""` attribute
    // never reaches the parser — same "not applied CSS" principle срезы 7/21
    // already give a blocked external `<link>`/inline `<style>`. The
    // attribute's raw text is untouched (`getAttribute('style')` still
    // returns it), only its effect on the cascade is suppressed.
    let inline_decls: Vec<Declaration> = if doc.is_style_attr_csp_blocked(node) {
        Vec::new()
    } else {
        doc.get(node)
            .get_attr("style")
            .filter(|s| !s.is_empty())
            .map(parse_inline_style)
            .unwrap_or_default()
    };

    // Собираем все matched declarations с их sort key:
    // (important, is_inline, layer_priority, specificity, rule_order, decl_index).
    //
    // `important` идёт первым: !important побеждает normal (CSS Cascade L4 §8.1).
    // `is_inline` — вторым: inline-style атрибут побеждает стилевой лист
    // (CSS Cascade L4 §6.4.3).
    // `layer_priority` — CSS Cascade L5 §6.4.5 @layer ordering:
    //   - normal: unlayered = N (highest), layer[i] = i (earlier layer = lower priority)
    //   - !important: unlayered = -N (lowest), layer[i] = -i (earlier layer = highest)
    //   Ascending sort, last applied wins → correct per spec.
    // `specificity`, `rule_idx`, `decl_idx` — обычный каскад внутри одного layer.
    let layer_n = sheet.layer_order.len() as i32;
    // Compute layer priority sign correctly for normal vs !important declarations.
    // For normal (imp=false): higher = wins → unlayered = N > layer[N-1] > ... > layer[0]
    // For !important (imp=true): lower layer_idx wins → layer[0] = 0 > layer[1] = -1 > ... > unlayered = -N
    let layer_pri = |imp: bool, layer_idx: i32| -> i32 {
        if imp { -layer_idx } else { layer_idx }
    };
    // (important, is_inline, layer_priority, specificity, rule_idx, decl_idx,
    // declaration, shadow origin). The last field tracks which shadow-tree
    // stylesheet (if any) a matched declaration physically came from — `None`
    // for the document `sheet` (including @layer/@media/@supports/@scope/
    // inline, all of which live in `sheet` too). Needed so `@apply` (below)
    // can resolve `@mixin` names against the SAME stylesheet the `@apply` was
    // written in, not always the document one — a shadow tree's own `<style>`
    // has its own `mixin_rules`, invisible to `sheet.mixin_rules` (BUG-518
    // mixin-shadow-dom follow-up).
    type MatchedDecl<'a> =
        (bool, bool, i32, Specificity, usize, usize, &'a Declaration, Option<&'a Stylesheet>);
    let mut matched: Vec<MatchedDecl> = Vec::new();


    // Build or reuse a per-stylesheet rule index (thread-local, keyed by
    // pointer+length). Amortised O(1): rebuilt only when the sheet changes.
    let node_data = doc.get(node);
    let node_tag = node_data.element_name().map_or("", |q| q.local.as_str());
    let node_id = node_data.get_attr("id");
    let class_attr = node_data.get_attr("class").unwrap_or("");
    let node_classes: Vec<&str> = class_attr.split_whitespace().collect();
    let node_attrs: &[lumen_dom::Attribute] = match &node_data.data {
        lumen_dom::NodeData::Element { attrs, .. } => attrs,
        _ => &[],
    };
    shareable &= is_svg_presentational_element(node_tag);

    ensure_cascade_index(sheet, viewport, dark_mode);
    let cands = with_front_cascade_index(|idx| {
        idx.rules.candidates(node_tag, node_id, &node_classes, node_attrs)
    });

    for &rule_idx in &cands {
        let rule = &sheet.rules[rule_idx];
        shareable &= rule.selectors.iter().all(selector_is_share_safe);
        let mut best: Option<Specificity> = None;
        for complex in &rule.selectors {
            if matches_complex(complex, doc, node) {
                let spec = complex.specificity();
                best = Some(match best {
                    Some(prev) if prev >= spec => prev,
                    _ => spec,
                });
            }
        }
        if let Some(spec) = best {
            for (decl_idx, decl) in rule.declarations.iter().enumerate() {
                let lp = layer_pri(decl.important, layer_n);
                matched.push((decl.important, false, lp, spec, rule_idx, decl_idx, decl, None));
            }
        }
    }

    // CSS Cascade L5 §6.4.5 — @layer rules: каждый LayerRule добавляет
    // свои декларации в каскад с layer_priority < unlayered. Layer с меньшим
    // индексом в `layer_order` имеет меньший приоритет для normal (earlier
    // declared → overridden by later), и больший для !important (CSS Cascade
    // L5 §6.4.5 inversion: earlier layer !important wins).
    let layer_rule_base = sheet.rules.len()
        + sheet.media_rules.iter().map(|m| m.rules.len()).sum::<usize>();
    // THREAD-4 срез 7: one candidate query over all `@layer` blocks (see
    // `CascadeIndex::layers`); `flat` is the rule's running offset across
    // blocks, i.e. exactly the old `layer_rule_offset + rule_idx`.
    let layer_cands = with_front_cascade_index(|idx| {
        idx.layers
            .candidates(node_tag, node_id, &node_classes, node_attrs)
            .into_iter()
            .filter_map(|flat| {
                let (block, rule_idx) = *idx.layer_rules.get(flat)?;
                if !*idx.layer_active.get(block)? {
                    return None;
                }
                Some((flat, block, rule_idx, *idx.layer_order_pos.get(block)?))
            })
            .collect::<Vec<_>>()
    });
    for (flat, block, rule_idx, layer_idx) in layer_cands {
        let Some(rule) = sheet.layers.get(block).and_then(|l| l.rules.get(rule_idx)) else {
            continue;
        };
        shareable &= rule.selectors.iter().all(selector_is_share_safe);
        let mut best: Option<Specificity> = None;
        for complex in &rule.selectors {
            if matches_complex(complex, doc, node) {
                let spec = complex.specificity();
                best = Some(match best {
                    Some(prev) if prev >= spec => prev,
                    _ => spec,
                });
            }
        }
        if let Some(spec) = best {
            let global_rule_idx = layer_rule_base + flat;
            for (decl_idx, decl) in rule.declarations.iter().enumerate() {
                let lp = layer_pri(decl.important, layer_idx);
                matched.push((decl.important, false, lp, spec, global_rule_idx, decl_idx, decl, None));
            }
        }
    }

    // CSS Media Queries L4: rules внутри `@media`-блока, чей query
    // совпадает с текущим MediaContext, добавляются в каскад. В Phase 0
    // упрощённый MediaContext: media_type="screen", width/height из
    // viewport. Source-order между обычными и
    // @media-rules не сохраняется идеально (все @media идут после
    // обычных) — это известное ограничение.
    //
    // Perf: "active" per block precomputed once per (sheet, viewport,
    // dark_mode) in `CascadeIndex::active_media` — see its doc comment.
    // `media.query.matches(..)` used to run here on every node. Fetched once
    // per node (not once per block) to avoid N thread-local accesses when
    // the stylesheet has many `@media` blocks.
    let active_media = with_front_cascade_index(|idx| idx.active_media.clone());
    let mut next_rule_idx = sheet.rules.len();
    for (media_i, media) in sheet.media_rules.iter().enumerate() {
        if !active_media[media_i] {
            next_rule_idx += media.rules.len();
            continue;
        }
        // BUG-284: candidate pre-filter (see @layer above) — real-world
        // stylesheets often put the bulk of their rules inside @media blocks.
        let media_cands = with_front_cascade_index(|idx| {
            idx.media[media_i].candidates(node_tag, node_id, &node_classes, node_attrs)
        });
        for rule_idx in media_cands {
            let rule = &media.rules[rule_idx];
            shareable &= rule.selectors.iter().all(selector_is_share_safe);
            let mut best: Option<Specificity> = None;
            for complex in &rule.selectors {
                if matches_complex(complex, doc, node) {
                    let spec = complex.specificity();
                    best = Some(match best {
                        Some(prev) if prev >= spec => prev,
                        _ => spec,
                    });
                }
            }
            if let Some(spec) = best {
                let global_rule_idx = next_rule_idx + rule_idx;
                for (decl_idx, decl) in rule.declarations.iter().enumerate() {
                    let lp = layer_pri(decl.important, layer_n);
                    matched.push((decl.important, false, lp, spec, global_rule_idx, decl_idx, decl, None));
                }
            }
        }
        next_rule_idx += media.rules.len();
    }
    // CSS Conditional Rules L3 §2 — `@supports`: evaluate condition against
    // Lumen's supported-properties list; include contained rules only when
    // condition is true (same ordering semantics as @media).
    //
    // Perf: "active" precomputed once per sheet in `CascadeIndex::active_supports`
    // (see doc comment) — `supports.condition.evaluate(..)` used to run per node.
    let active_supports = with_front_cascade_index(|idx| idx.active_supports.clone());
    for (supports_i, supports) in sheet.supports_rules.iter().enumerate() {
        if !active_supports[supports_i] {
            next_rule_idx += supports.rules.len();
            continue;
        }
        let supports_cands = with_front_cascade_index(|idx| {
            idx.supports[supports_i].candidates(node_tag, node_id, &node_classes, node_attrs)
        });
        for rule_idx in supports_cands {
            let rule = &supports.rules[rule_idx];
            shareable &= rule.selectors.iter().all(selector_is_share_safe);
            let mut best: Option<Specificity> = None;
            for complex in &rule.selectors {
                if matches_complex(complex, doc, node) {
                    let spec = complex.specificity();
                    best = Some(match best {
                        Some(prev) if prev >= spec => prev,
                        _ => spec,
                    });
                }
            }
            if let Some(spec) = best {
                let global_rule_idx = next_rule_idx + rule_idx;
                for (decl_idx, decl) in rule.declarations.iter().enumerate() {
                    let lp = layer_pri(decl.important, layer_n);
                    matched.push((decl.important, false, lp, spec, global_rule_idx, decl_idx, decl, None));
                }
            }
        }
        next_rule_idx += supports.rules.len();
    }
    // CSS Cascade L6 §5 — @scope rules: apply only when node is in scope.
    for scope_rule in &sheet.scope_rules {
        // Donut scoping (§3): `node` is in scope when it is an inclusive
        // descendant of the scope root but *not* of a scope limit that lies
        // within that same root subtree. `node_in_scope` resolves root and
        // limit together (nearest boundary wins) so a limit-matching element
        // *above* the root no longer removes the node from scope.
        if !node_in_scope(doc, node, &scope_rule.root, scope_rule.limit.as_deref()) {
            next_rule_idx += scope_rule.rules.len();
            continue;
        }
        for rule in &scope_rule.rules {
            let mut best: Option<Specificity> = None;
            for complex in &rule.selectors {
                if matches_complex(complex, doc, node) {
                    let spec = complex.specificity();
                    best = Some(match best {
                        Some(prev) if prev >= spec => prev,
                        _ => spec,
                    });
                }
            }
            if let Some(spec) = best {
                for (decl_idx, decl) in rule.declarations.iter().enumerate() {
                    let lp = layer_pri(decl.important, layer_n);
                    matched.push((decl.important, false, lp, spec, next_rule_idx, decl_idx, decl, None));
                }
            }
            next_rule_idx += 1;
        }
    }
    // CSS Scoping L1 §6.1-6.2 — shadow-tree-scoped style rules. `:host`/`:host()`
    // and `::slotted()` only have effect when written *inside* a shadow tree's own
    // stylesheet (collected per-host in `SHADOW_SHEETS`); the same selectors in the
    // page's document `<style>` are no-ops. Two scopes touch this node:
    //   (a) the node is itself a shadow host → its OWN shadow sheet's `:host` rules
    //       cascade onto it (the host lives in the light tree, so only `:host`-bearing
    //       rules from its shadow reach it);
    //   (b) the node is a slotted light child → its host's shadow sheet's `::slotted()`
    //       rules cascade onto it.
    // These declarations join the same cascade as document author rules; we give them
    // `rule_idx` values past the document range so source order stays stable (shadow
    // markup follows the head `<style>` in document order).
    // Clone the relevant shadow sheets out of the thread-local into locals that
    // live for the rest of this function, so the `&Declaration` references pushed
    // into `matched` outlive the (closure-scoped) thread-local borrow.
    let any_shadow = SHADOW_SHEETS.with(|c| !c.borrow().is_empty());
    let own_shadow: Option<Stylesheet> = if any_shadow && doc.is_shadow_host(node) {
        SHADOW_SHEETS.with(|c| c.borrow().get(&node).cloned())
    } else {
        None
    };
    let host_shadow: Option<Stylesheet> = if any_shadow {
        doc.get(node)
            .parent
            .filter(|&p| doc.is_shadow_host(p))
            .and_then(|host| SHADOW_SHEETS.with(|c| c.borrow().get(&host).cloned()))
    } else {
        None
    };
    // (a) `:host` / `:host(sel)` from the node's own shadow tree apply to the host.
    if let Some(ref shadow) = own_shadow {
        SHADOW_HOST_SCOPE.with(|c| c.set(node.index() as u32));
        for (i, rule) in shadow.rules.iter().enumerate() {
            let mut best: Option<Specificity> = None;
            for complex in &rule.selectors {
                if complex_has_host(complex) && matches_complex(complex, doc, node) {
                    let spec = complex.specificity();
                    best = Some(match best {
                        Some(prev) if prev >= spec => prev,
                        _ => spec,
                    });
                }
            }
            if let Some(spec) = best {
                let gidx = next_rule_idx + i;
                for (decl_idx, decl) in rule.declarations.iter().enumerate() {
                    let lp = layer_pri(decl.important, layer_n);
                    matched.push((decl.important, false, lp, spec, gidx, decl_idx, decl, Some(shadow)));
                }
            }
        }
        SHADOW_HOST_SCOPE.with(|c| c.set(u32::MAX));
    }
    // (b) `::slotted(sel)` from this node's host's shadow tree apply to the slotted child.
    if let Some(ref shadow) = host_shadow {
        let base = next_rule_idx + shadow.rules.len();
        for (i, rule) in shadow.rules.iter().enumerate() {
            let mut best: Option<Specificity> = None;
            for complex in &rule.selectors {
                if let Some(spec) = matches_slotted_complex(complex, doc, node) {
                    best = Some(match best {
                        Some(prev) if prev >= spec => prev,
                        _ => spec,
                    });
                }
            }
            if let Some(spec) = best {
                let gidx = base + i;
                for (decl_idx, decl) in rule.declarations.iter().enumerate() {
                    let lp = layer_pri(decl.important, layer_n);
                    matched.push((decl.important, false, lp, spec, gidx, decl_idx, decl, Some(shadow)));
                }
            }
        }
    }
    // (c) `node` lives inside a shadow tree (its DOM parent chain reaches a
    // `ShadowRoot` before reaching the document root) — that tree's own
    // regular (non-`:host`/`::slotted`) selectors apply to it. This is CSS
    // Scoping L1 §6's core case ("author a component's internal styles once,
    // scoped to its own tree"), distinct from (a)/(b) above, which only cover
    // the two *boundary*-crossing selector forms. Plain `matches_complex`
    // naturally excludes `:host` (gated on `SHADOW_HOST_SCOPE`, never set to
    // `node`'s index here) and `::slotted()` (a pseudo-element; `matches_simple`
    // always rejects `SimpleSelector::PseudoElement`), so no extra filtering
    // is needed to keep (a)/(b) semantics from being duplicated here.
    let interior_shadow: Option<Stylesheet> = if any_shadow {
        doc.enclosing_shadow_host(node)
            .and_then(|host| SHADOW_SHEETS.with(|c| c.borrow().get(&host).cloned()))
    } else {
        None
    };
    // THREAD-4 срез 4: shadow disqualification is contextual, not
    // document-wide. `own_shadow`/`host_shadow`/`interior_shadow` above
    // already answer the only question that matters for `node` itself — is
    // it a shadow host, a slotted light child, or inside a shadow tree? A
    // node that is none of the three never touches `SHADOW_HOST_SCOPE` or a
    // shadow stylesheet during its own cascade above, so an unrelated
    // shadow tree elsewhere in the document cannot change its result.
    shareable &= own_shadow.is_none() && host_shadow.is_none() && interior_shadow.is_none();
    if let Some(ref shadow) = interior_shadow {
        let base = next_rule_idx
            + own_shadow.as_ref().map_or(0, |s| s.rules.len())
            + host_shadow.as_ref().map_or(0, |s| s.rules.len());
        for (i, rule) in shadow.rules.iter().enumerate() {
            let mut best: Option<Specificity> = None;
            for complex in &rule.selectors {
                if matches_complex(complex, doc, node) {
                    let spec = complex.specificity();
                    best = Some(match best {
                        Some(prev) if prev >= spec => prev,
                        _ => spec,
                    });
                }
            }
            if let Some(spec) = best {
                let gidx = base + i;
                for (decl_idx, decl) in rule.declarations.iter().enumerate() {
                    let lp = layer_pri(decl.important, layer_n);
                    matched.push((decl.important, false, lp, spec, gidx, decl_idx, decl, Some(shadow)));
                }
            }
        }
    }

    // Inline-style declarations подключаются с `is_inline = true` и
    // synthetic specificity = default (Cascade L4 §6.4.3 — реальная
    // specificity inline-стиля игнорируется в сортировке: за порядок
    // отвечает is_inline-бит, а внутри inline — источниковый порядок
    // декларации в атрибуте). Inline-стиль всегда unlayered.
    for (decl_idx, decl) in inline_decls.iter().enumerate() {
        matched.push((
            decl.important,
            true,
            layer_pri(decl.important, layer_n),
            Specificity::default(),
            next_rule_idx,
            decl_idx,
            decl,
            None,
        ));
    }
    // GAP-UASHADOWSLOT: CSS Scoping L1 §3.1 — a document rule never reaches
    // into a shadow tree, and a UA shadow tree has no sheet of its own, so a
    // UA slot keeps exactly the UA styles set above. (Author shadow trees
    // still see document rules here — a wider, pre-existing gap.)
    if doc.ua_slot_role(node).is_some() {
        matched.clear();
    }
    matched.sort_by_key(|&(imp, inline, lp, spec, rule_idx, decl_idx, _, _)| {
        (imp, inline, lp, spec, rule_idx, decl_idx)
    });
    drop(prof_match);
    let prof_revert = lumen_core::profile::scope_detail("cs_revert_prepass");

    // CSS Cascade L5 §6.4.6 — `revert-layer`: a declaration whose value is
    // `revert-layer` rolls the cascaded value back to what it would be if all
    // declarations of that property in the *current* cascade layer (same
    // importance) were removed. CSS Cascade L5 §revert-rule-keyword (BUG-487)
    // — `revert-rule` rolls the value back to what it would be if the one
    // style rule (or the inline `style` attribute, which shares a single
    // synthetic rule index for all its declarations) that contributed the
    // winning declaration didn't exist, regardless of layer/origin/importance.
    // Both are resolved as a pre-pass over the already cascade-sorted
    // `matched` set: for every property whose winning declaration (the last
    // occurrence in sort order) is `revert-layer`/`revert-rule`, drop every
    // declaration of that property belonging to the winning layer/rule
    // respectively, then repeat. Repetition matters for two reasons: a lower
    // layer may itself contain `revert-layer`, and resolving one keyword can
    // reveal the *other* as the new winner (`revert-rule-revert-layer.html`
    // chains both), so every round rechecks for both. The normal last-wins
    // apply loop below then yields the reverted value automatically; when
    // nothing remains the property keeps its inherited/initial value.
    //
    // Neither `revert-layer` nor `revert-rule` is a `CssWideKeyword`: one
    // depends on the declaration's own layer, the other on its own rule, so
    // neither can be applied per-declaration like `inherit`/`initial`.
    // Shorthand↔longhand reverts across layers/rules are a known limitation
    // (grouping is by exact property name).
    //
    // BUG-341 S10: the loop below allocates a lowercased `String` key per
    // matched declaration plus a `HashMap` just to discover, on essentially
    // every element of every real page, that nothing declares `revert-layer`/
    // `revert-rule`. One allocation-free scan first (measured: 1.4 ms per
    // chrome layout pass, ~7% of the cascade stage).
    while matched.iter().any(|&(_, _, _, _, _, _, decl, _)| {
        let v = decl.value.trim();
        v.eq_ignore_ascii_case("revert-layer") || v.eq_ignore_ascii_case("revert-rule")
    }) {
        use std::collections::HashMap;
        // Winner per property = last occurrence in the cascade-sorted vec.
        // (lp, important, rule_idx, is_revert_layer, is_revert_rule)
        let mut winners: HashMap<String, (i32, bool, usize, bool, bool)> = HashMap::new();
        for &(imp, _inline, lp, _, rule_idx, _, decl, _) in &matched {
            let key = decl.property.to_ascii_lowercase();
            let v = decl.value.trim();
            let is_revert_layer = v.eq_ignore_ascii_case("revert-layer");
            let is_revert_rule = v.eq_ignore_ascii_case("revert-rule");
            winners.insert(key, (lp, imp, rule_idx, is_revert_layer, is_revert_rule));
        }
        let layer_targets: Vec<(String, i32, bool)> = winners
            .iter()
            .filter(|&(_, &(_, _, _, is_revert_layer, _))| is_revert_layer)
            .map(|(k, &(lp, imp, _, _, _))| (k.clone(), lp, imp))
            .collect();
        let rule_targets: Vec<(String, usize)> = winners
            .iter()
            .filter(|&(_, &(_, _, _, _, is_revert_rule))| is_revert_rule)
            .map(|(k, &(_, _, rule_idx, _, _))| (k.clone(), rule_idx))
            .collect();
        if layer_targets.is_empty() && rule_targets.is_empty() {
            break;
        }
        matched.retain(|&(imp, _inline, lp, _, rule_idx, _, decl, _)| {
            let key = decl.property.to_ascii_lowercase();
            let hit_layer = layer_targets
                .iter()
                .any(|(tk, tlp, timp)| *tk == key && *tlp == lp && *timp == imp);
            let hit_rule =
                rule_targets.iter().any(|(tk, tridx)| *tk == key && *tridx == rule_idx);
            !(hit_layer || hit_rule)
        });
    }

    // CSS Cascade L4 §7.4 — `revert` откатывается к значению «как если бы
    // author/user-правил не было». `style` прямо здесь уже содержит ровно
    // это: наследуемые поля скопированы из `inherited`, а все `ua_*`/
    // `apply_ua_*`/presentational-hint пассы выше (§ «UA stylesheet» /
    // «HTML presentational hints») отработали, но ни одна matched-декларация
    // ещё не применена. Снэпшот уходит в `apply_declaration` → `apply_css_wide_keyword`.
    //
    // Perf (docs/tasks/p3-cascade-perf.md Задача 1): безусловный
    // `ComputedStyle::clone()` здесь был вторым по весу вкладом в build_box
    // на тяжёлых страницах — на каждый узел клонируются десятки Vec/String/
    // HashMap-полей ради свойства, которое почти никогда не встречается в
    // реальном CSS. Клонируем, только если среди matched-деклараций реально
    // есть `revert` — прямой (`prop: revert`) или через цепочку custom
    // properties (`--x: revert; prop: var(--x);`, в т.ч. унаследованную от
    // предка — все такие декларации остаются raw-строками в
    // `custom_props`/`inherited.custom_props`, поэтому проверка ловит любую
    // глубину вложенности). Когда клон не нужен, `ua_baseline_ref` указывает
    // на `inherited` как безопасную заглушку: `apply_declaration` читает этот
    // параметр только внутри ветки `kw == Revert`, которая в этом случае
    // гарантированно не сработает ни для одной декларации.
    let ua_baseline_font_size = style.font_size;
    let needs_ua_baseline = matched.iter().any(|&(_, _, _, _, _, _, decl, _)| {
        decl.value.trim().eq_ignore_ascii_case("revert")
    }) || (
        matched.iter().any(|&(_, _, _, _, _, _, decl, _)| decl.value.contains("var("))
            && inherited.custom_props.values().any(|v| v.trim().eq_ignore_ascii_case("revert"))
    );
    let ua_baseline_storage: Option<ComputedStyle> = needs_ua_baseline.then(|| style.clone());
    let ua_baseline_ref: &ComputedStyle = ua_baseline_storage.as_ref().unwrap_or(inherited);
    drop(prof_revert);
    let prof_apply = lumen_core::profile::scope_detail("cs_apply");

    // Custom-properties pass: все `--name: value` декларации применяются
    // отдельно и ДО остальных пассов, чтобы любая обычная декларация могла
    // видеть финальное значение custom property независимо от порядка
    // объявления в source. Каскад уже соблюдён через sort `matched`:
    // последующая запись с тем же ключом перебивает раннюю.
    //
    // BUG-731: пасс стоит ПЕРЕД font-size-pre-pass, а не после него. Иначе
    // `font-size: var(--x)` / `font: var(--x)` видели бы только унаследованную
    // карту, а собственное объявление элемента (`.card { --fs: 20px;
    // font-size: var(--fs) }`) — нет. Пасс ни от чего в pre-pass-ах не зависит:
    // он читает только `matched` + `registry`, а `validate_against_syntax`
    // работает по тексту значения, не по computed font-size.
    //
    // CSS Properties and Values L1 §1.1 «invalid at computed value time»:
    // для зарегистрированных custom properties value валидируется против
    // `syntax`-дескриптора. Невалидное значение игнорируется — старое
    // значение (родительское inherited или initial-value) остаётся.
    // value, содержащее `var(`, пропускается без валидации — резолв
    // происходит позже, и итоговая строка может быть валидной.
    for (_, _, _, _, _, _, decl, _) in &matched {
        if let Some(name) = decl.property.strip_prefix("--") {
            let key = format!("--{name}");
            if let Some(prop_rule) = registry.get(key.as_str())
                && !decl.value.contains("var(")
                && !validate_against_syntax(&decl.value, &prop_rule.syntax)
            {
                // Invalid at computed value time — skip declaration.
                continue;
            }
            style.custom_props.make_mut().insert(key, decl.value.clone());
        }
    }

    // CSS Properties and Values L1 §1.1: для каждого зарегистрированного
    // имени, у которого после custom-pass нет значения (ни унаследованного,
    // ни локально объявленного), подставить `initial-value`. Делается до
    // остальных пассов, чтобы `var(--registered)` в обычных декларациях
    // видел initial-value-fallback.
    apply_property_initial_values(&mut style.custom_props, &registry);

    // Pre-pass: применяем font-size раньше, потому что em/% других свойств
    // считаются относительно computed font-size этого же элемента, а em для
    // самого font-size — относительно inherited (родительского) font-size.
    // Pre-pass: `zoom` (CSS Viewport L1 §5) must be known before font-size and
    // before any other length is resolved, because it multiplies all of them.
    // `matched` is cascade-sorted, so the last parseable declaration wins.
    let mut own_zoom = 1.0f32;
    for (_, _, _, _, _, _, decl, _) in &matched {
        if decl.property.eq_ignore_ascii_case("zoom")
            && let Some(z) = parse_zoom(&decl.value, inherited.font_size)
        {
            own_zoom = z;
        }
        // `all: initial|unset|revert` сбрасывает и `zoom` (не наследуется,
        // initial и UA-значение — 1). `all: inherit` взял бы собственный
        // множитель родителя, которого стиль не хранит — оставляем как есть.
        if decl.property == "all"
            && parse_css_wide_keyword(decl.value.trim()).is_some_and(|kw| kw != CssWideKeyword::Inherit)
        {
            own_zoom = 1.0;
        }
    }
    style.effective_zoom = inherited.effective_zoom * own_zoom;

    let parent_fs = inherited.font_size;
    let is_quirks = doc.mode() == DocumentMode::Quirks;
    // Which basis the winning font-size resolved against decides the zoom factor
    // below. No declaration applies → the value is the inherited (or UA-hinted
    // `em`) one, i.e. parent-relative.
    let mut fs_basis = FontSizeBasis::ParentRelative;
    for (_, _, _, _, _, _, decl, _) in &matched {
        if let Some(basis) =
            apply_font_size(&mut style, decl, parent_fs, ua_baseline_font_size, viewport, is_quirks)
        {
            fs_basis = basis;
        }
    }

    // A font-size resolved from a zoom-independent basis (`16px`, `rem`, …) has
    // not been scaled by anyone, so it takes the full compounded factor. One
    // resolved against the parent's size (`em`, `%`, or plain inheritance)
    // already carries every ancestor's zoom and needs only this element's own
    // contribution — applying `effective_zoom` to it would re-apply the
    // ancestors', once per level of nesting.
    style.font_size *= match fs_basis {
        FontSizeBasis::Absolute => style.effective_zoom,
        FontSizeBasis::ParentRelative => own_zoom,
    };
    // The document element's computed font-size is what `rem` refers to.
    if doc.get(node).parent == Some(doc.root()) {
        style.root_font_size = style.font_size;
        style.root_zoom = style.effective_zoom;
    }

    // Pre-pass: применяем color-scheme раньше main-pass, чтобы системные
    // цвета (Canvas, ButtonFace, …) резолвились против правильной темы
    // ещё в ходе main-pass (для поля `color: Color`; CssColor-поля
    // резолвятся отдельным post-pass в конце compute_style).
    for (_, _, _, _, _, _, decl, _) in &matched {
        if decl.property.eq_ignore_ascii_case("color-scheme") {
            apply_declaration(&mut style, decl, parent_fs, viewport, FontWeight::NORMAL, inherited, ua_baseline_ref, is_quirks, dark_mode);
        }
    }

    // Main-pass: остальные декларации; em-basis теперь = current font_size.
    // Inherited font_weight нужен для разрешения `lighter`/`bolder`;
    // `inherited` целиком — для CSS-wide keywords (CSS Cascade L4 §7).
    let em_basis = style.font_size;
    let parent_weight = inherited.font_weight;

    // SVG 2 §6.4: presentation attributes act as author rules of the lowest
    // priority. Apply them before the matched-declaration loop so any CSS rule
    // (stylesheet or inline) overrides them.
    apply_svg_presentational_hints(
        doc, node, &mut style, em_basis, viewport, parent_weight, inherited, is_quirks,
    );

    // CSS Basic UI L4 §5 — pre-scan the cascade-winning `appearance` value
    // (matched is cascade-sorted; later = higher priority, inline included) so
    // that `appearance: none` strips UA-default border/background/padding
    // *before* the author cascade. Stripping after the cascade clobbered
    // author-specified border/background/padding (BUG-211).
    let mut appearance_none = false;
    for (_, _, _, _, _, _, decl, _) in &matched {
        match decl.property.as_str() {
            "appearance" | "-webkit-appearance" | "-moz-appearance" => {
                appearance_none = decl.value.trim().eq_ignore_ascii_case("none");
            }
            _ => {}
        }
    }
    if appearance_none {
        strip_ua_appearance_box_styling(doc, node, &mut style);
    }

    // CSS Scoping L1 §3.5 (BUG-519): `@function` names are tree-scoped. A
    // declaration from a shadow tree's stylesheet resolves `--fn()` against
    // that tree's own `@function`s first, then each enclosing tree's, then
    // the document's (`sheet.function_rules`, appended at the use site). One
    // chain per shadow sheet that contributed to `matched`, keyed by the
    // host that owns the sheet; built only when some tree in it actually
    // declares a function, so pages without shadow `@function`s pay nothing.
    let own_fn_chain = own_shadow.as_ref().and_then(|s| shadow_function_chain(doc, s, node));
    let host_fn_chain = host_shadow
        .as_ref()
        .zip(doc.get(node).parent)
        .and_then(|(s, host)| shadow_function_chain(doc, s, host));
    let interior_fn_chain = interior_shadow
        .as_ref()
        .zip(doc.enclosing_shadow_host(node))
        .and_then(|(s, host)| shadow_function_chain(doc, s, host));

    for (_, _, _, _, _, _, decl, shadow_origin) in &matched {
        // CSS Cascade L5 §6.4.6 / §revert-rule-keyword: a `revert-layer`/
        // `revert-rule` declaration that survived the pre-pass was overridden
        // by a higher layer/rule for the same property, so it has no effect —
        // skip it instead of letting it fail property parsing.
        let dv = decl.value.trim();
        if dv.eq_ignore_ascii_case("revert-layer") || dv.eq_ignore_ascii_case("revert-rule") {
            continue;
        }
        // CSS Functions and Mixins L1: `@apply` markers (`MIXIN_APPLY_MARKER`,
        // pushed by the parser at the exact source position of the `@apply`
        // statement) are not a real property/value pair — expand them into
        // zero or more real declarations and apply each in place instead of
        // falling into the attr()/var()/function pipeline below, which would
        // misparse the marker's raw-text payload. Gated on `mixin_rules`
        // being non-empty, same reasoning as the `function_rules` gate below.
        if decl.property == MIXIN_APPLY_MARKER {
            // A `@apply` written inside a shadow tree's own `<style>` must see
            // that tree's OWN `@mixin`s first — `SHADOW_SHEETS[host]` is a
            // stylesheet the document-level `sheet.mixin_rules` never sees
            // (BUG-518 mixin-shadow-dom follow-up, `mixin-shadow-dom.html`'s
            // "Style in shadow DOM should have access to inside mixins").
            // Falling back to the document's own mixins afterwards keeps the
            // opposite direction working too (an outer mixin, invoked from
            // inside a shadow tree, `mixin-shadow-dom.html`'s "...to outside
            // non-adopted mixins") — `expand_apply_rule`'s name lookup takes
            // the first match, so listing the shadow's own rules first gives
            // them precedence over a same-named outer one, matching how a
            // shadow tree's own declarations already shadow inherited ones.
            let combined_mixins: Vec<MixinRule>;
            let mixins: &[MixinRule] = match shadow_origin {
                Some(shadow) if !shadow.mixin_rules.is_empty() => {
                    combined_mixins = shadow
                        .mixin_rules
                        .iter()
                        .chain(sheet.mixin_rules.iter())
                        .cloned()
                        .collect();
                    &combined_mixins
                }
                _ => &sheet.mixin_rules,
            };
            if !mixins.is_empty()
                && let Some(expanded) = expand_mixin_apply(
                    &decl.value,
                    mixins,
                    &sheet.layer_order,
                    &sheet.function_rules,
                    &style.custom_props,
                    0,
                    em_basis,
                    viewport,
                )
            {
                for d in &expanded {
                    // BUG-1010: a mixin's `@result` block can itself set a
                    // custom property — `apply_declaration` ignores `--`-prefixed
                    // properties (they have their own pass), so write the
                    // already-fully-resolved value straight into `custom_props`
                    // instead of silently dropping it.
                    if let Some(name) = d.property.strip_prefix("--") {
                        style.custom_props.make_mut().insert(format!("--{name}"), d.value.clone());
                    } else {
                        apply_declaration(
                            &mut style, d, em_basis, viewport, parent_weight, inherited,
                            ua_baseline_ref, is_quirks, dark_mode,
                        );
                    }
                }
            }
            continue;
        }
        // BUG-1010: tracks whether `effective_decl` below actually differs from
        // `decl` (an `attr()`/`--fn()` expansion happened) — a registered
        // custom property (`@property` with a `syntax`) that FAILED the
        // pre-pass's `validate_against_syntax` check was deliberately skipped
        // there (its old/initial value stands), so an untouched `decl` must
        // not be re-inserted into `custom_props` here, or this pass would
        // silently undo that rejection for every `--`-prefixed declaration.
        let mut custom_prop_expanded = false;
        // CSS Values L4 §7.7: expand attr() typed references before applying.
        let attr_buf;
        let effective_decl: &Declaration = if decl.value.contains("attr(") {
            let Some(v) = expand_attr_val(&decl.value, doc, node) else { continue };
            custom_prop_expanded = true;
            attr_buf = Declaration { property: decl.property.clone(), value: v, important: decl.important };
            &attr_buf
        } else {
            decl
        };
        // CSS Functions and Mixins L1: expand `--name(<args>)` custom function
        // calls before applying. `var(` is resolved first (against the same
        // `style.custom_props` `apply_declaration` would use) so a call reached
        // indirectly through a custom property (`--gap: --double(5px); width:
        // var(--gap);`) is visible to the call-site scanner, not just direct
        // calls (`width: --double(5px);`). Gated on `function_rules` being
        // non-empty — pages without `@function` pay nothing extra here, and
        // `apply_declaration`'s own `var()` pass below is then a no-op.
        let func_buf;
        let fn_chain = shadow_origin.and_then(|origin| {
            [(&own_shadow, &own_fn_chain), (&host_shadow, &host_fn_chain), (&interior_shadow, &interior_fn_chain)]
                .into_iter()
                .find(|(s, _)| s.as_ref().is_some_and(|s| std::ptr::eq(s, origin)))
                .and_then(|(_, chain)| chain.as_ref())
        });
        let doc_fn_scope = [sheet.function_rules.as_slice()];
        let chain_fn_scopes: Vec<&[FunctionRule]>;
        let fn_scopes: &[&[FunctionRule]] = match fn_chain {
            Some(chain) => {
                chain_fn_scopes = chain
                    .iter()
                    .map(Vec::as_slice)
                    .chain(std::iter::once(sheet.function_rules.as_slice()))
                    .collect();
                &chain_fn_scopes
            }
            None => &doc_fn_scope,
        };
        let effective_decl: &Declaration = if fn_scopes.iter().any(|s| !s.is_empty())
            && effective_decl.value.contains("--")
        {
            let pre = if effective_decl.value.contains("var(") {
                match expand_vars(&effective_decl.value, &style.custom_props, 0, em_basis, viewport) {
                    Some(v) => v,
                    None => continue,
                }
            } else {
                effective_decl.value.clone()
            };
            match expand_custom_functions_scoped(&pre, fn_scopes, &style.custom_props, 0, em_basis, viewport) {
                Some(v) => {
                    custom_prop_expanded = true;
                    func_buf = Declaration {
                        property: effective_decl.property.clone(),
                        value: v,
                        important: effective_decl.important,
                    };
                    &func_buf
                }
                None => continue,
            }
        } else {
            effective_decl
        };
        // BUG-1010: `effective_decl` above already resolved `attr()`/`--fn()`
        // for this declaration's own value (the same pipeline typed properties
        // get) — but `apply_declaration` ignores `--`-prefixed properties
        // entirely (they have their own pre-pass), so that resolved value was
        // silently discarded and only the pre-pass's raw text ever reached
        // `custom_props`. Write it back here instead, overwriting the raw
        // pre-pass entry for this property with the point-of-declaration
        // resolved one. Gated on `custom_prop_expanded`: a declaration that
        // went untouched by both branches above (plain literal, or a bare
        // `var()`/`env()` chain — BUG-499 already finishes those off later via
        // `expand_vars_and_env` at snapshot time) must not be re-inserted
        // here, or this would silently undo the pre-pass's syntax-validation
        // rejection for a registered (`@property`) custom property.
        if custom_prop_expanded && let Some(name) = effective_decl.property.strip_prefix("--") {
            style.custom_props.make_mut().insert(format!("--{name}"), effective_decl.value.clone());
        } else if !effective_decl.property.starts_with("--") {
            apply_declaration(&mut style, effective_decl, em_basis, viewport, parent_weight, inherited, ua_baseline_ref, is_quirks, dark_mode);
        }
    }

    // CSS Display L3 §2.7 — blockification: the root element's own box can
    // never be eliminated (there is nothing above it to splice its children
    // into), so a `display: contents` document element computes to `block`
    // instead. Runs after the cascade loop so it sees the final `display`.
    if style.display == Display::Contents && doc.document_element() == Some(node) {
        style.display = Display::Block;
    }

    // WHATWG Compat §2.1 — legacy `display: -webkit-box`/`-webkit-inline-box` is laid out as a
    // flex container whose axis is `-webkit-box-orient` (Gap Decorations L1 applies to it, WPT
    // `css-gaps/flex/webkit-box.tentative`). The original keyword is kept in `legacy_box_display`
    // for `getComputedStyle`. A box that clamps its lines (`-webkit-line-clamp`/`continue: discard`)
    // stays a block — that is the ellipsis idiom and its children are inline text, not items.
    // A `<fieldset>` also keeps the block path: its content lives in an anonymous box the flex
    // arm does not build (`compat/webkit-box-fieldset`: the child must span the width).
    if matches!(style.display, Display::WebkitBox | Display::WebkitInlineBox)
        && style.line_clamp.is_none()
        && style.continue_value != CssContinue::Discard
        && !matches!(&doc.get(node).data, NodeData::Element { name, .. } if name.local.as_str() == "fieldset")
    {
        style.legacy_box_display = Some(style.display);
        style.display = if style.display == Display::WebkitBox { Display::Flex } else { Display::InlineFlex };
        style.flex_direction = if style.box_orient == WebkitBoxOrient::Vertical {
            FlexDirection::Column
        } else {
            FlexDirection::Row
        };
    }

    // CSS Align L3 §vertical-align / Flexbox §4, Grid §6 — an in-flow child of a
    // flex or grid container is blockified, and `vertical-align` only applies
    // to inline-level boxes and table cells: the computed value has no effect
    // on a flex/grid item, so its text must not be shifted by it (WPT
    // `flex-item-vertical-align.html`).
    if matches!(
        inherited.display,
        Display::Flex | Display::InlineFlex | Display::Grid | Display::InlineGrid
    ) && style.display != Display::TableCell
    {
        style.vertical_align = crate::style::VerticalAlign::Baseline;
    }

    // CSS Color 4 §6.2 — post-pass: resolve any CssColor::System variants in
    // CssColor-typed fields (border-color, background-color, etc.) now that
    // style.color_scheme is final. The `color` field (Color, not CssColor) was
    // already resolved inline in the `"color"` branch of apply_declaration.
    drop(prof_apply);
    let _prof_post = lumen_core::profile::scope_detail("cs_post");
    resolve_system_colors_in_style(&mut style, dark_mode);

    // CSS Color Adjustment L1 §3 — Forced Colors Mode: when the user preference
    // is active, override author colors with the forced system palette
    // (respecting `forced-color-adjust`). Runs after system-color resolution so
    // it sees final Rgba values and after the full cascade so it sees the final
    // `forced-color-adjust` value.
    if forced_colors_active() {
        apply_forced_colors_mode(doc, node, &mut style, dark_mode);
    }

    // CSS Overflow L3 §logical (BUG-505) — resolve `overflow-block`/
    // `overflow-inline` to `overflow_x`/`overflow_y` before the axis-pair
    // adjustment below, so the adjustment sees the final physical pair, not
    // a stale one. Must run before `resolve_logical_properties` too, since
    // that call has no knowledge of overflow's writing-mode-dependent
    // axis swap.
    resolve_overflow_logical_properties(&mut style);

    // CSS Overflow L3 §2.1: if one axis is `visible` and the other is not,
    // the `visible` axis becomes `auto` (both axes must agree on visibility).
    (style.overflow_x, style.overflow_y) = coerce_overflow_axes(style.overflow_x, style.overflow_y);

    // HTML LS §obsolete (BUG-605): `<marquee>` forces `overflow: hidden` in
    // the UA stylesheet as `!important`, unconditionally overriding any
    // author `overflow` (even inline `style=""`) — a pre-cascade UA hint
    // would still lose to author declarations, so this runs post-cascade
    // like `apply_forced_colors_mode` above.
    if let NodeData::Element { name, .. } = &doc.get(node).data
        && name.local.as_str() == "marquee"
    {
        style.overflow_x = Overflow::Hidden;
        style.overflow_y = Overflow::Hidden;
    }

    // CSS Overscroll Behavior L1 §2 (BUG-516) — resolve `overscroll-behavior-
    // block`/`-inline` to `overscroll_behavior_x`/`_y`, same writing-mode
    // axis swap as `overflow-block`/`-inline` above.
    resolve_overscroll_behavior_logical_properties(&mut style);

    // CSS Logical Properties L1 — resolve logical properties to physical.
    resolve_logical_properties(&mut style);

    // CSS Backgrounds L3 §4.2 — the computed `border-*-width` is `0` when the side's
    // `border-style` is `none` or `hidden`: `border-width: 10px` without a style draws
    // nothing and takes no room (WPT `grid-baseline-004`: `.style3 { border-width: … }`
    // on an element that never gets `border-style`). Runs after logical → physical so
    // `border-block-*` sides are covered too.
    zero_unstyled_border_widths(&mut style);

    // CSS Basic UI L4 §4.4 — field-sizing: content post-pass.
    // apply_ua_form_controls ran before the cascade and may have set explicit UA
    // dimensions. Now that field_sizing is final, clear width/height for text-entry
    // controls so lay_out picks up field_sizing_content_intrinsic dimensions instead.
    if style.field_sizing == FieldSizing::Content {
        apply_ua_form_controls_field_sizing_clear(doc, node, &mut style);
    }

    // CSS Fonts L4 §13 — resolve `font-palette: <dashed-ident>` against the
    // stylesheet's `@font-palette-values` rules now: paint builds the display
    // list from ComputedStyle alone and has no stylesheet access. Runs after
    // the full cascade so it sees the final `font-palette` and `font-family`.
    style.font_palette_resolved = match &style.font_palette {
        FontPalette::Custom(name) => resolve_font_palette_overrides(
            &sheet.font_palette_values,
            name,
            style.font_family.first().map(String::as_str).unwrap_or(""),
        ),
        _ => None,
    };

    apply_webkit_scrollbar_pseudos(doc, node, sheet, &mut style, viewport, dark_mode);

    // Last, so every earlier pass has already written its box-model lengths and
    // each is scaled exactly once. `font_size` was handled next to the cascade's
    // font-size pre-pass and is deliberately not re-scaled here.
    let z = style.effective_zoom;
    // On the document element itself `rem` still means the initial 16px (CSS
    // Values L4 §5.1.2), zoomed like any other absolute length.
    let rem_k = if doc.get(node).parent == Some(doc.root()) {
        z
    } else {
        style.root_font_size / crate::style::ROOT_FONT_SIZE * (z / style.root_zoom)
    };
    let root_k = z / style.root_zoom;
    apply_zoom_to_lengths(&mut style, z, rem_k, root_k);

    (style, shareable)
}

/// CSS Backgrounds L3 §4.2: a side whose `border-style` is `none`/`hidden` has a computed
/// width of `0` whatever `border-width` says.
fn zero_unstyled_border_widths(style: &mut ComputedStyle) {
    use crate::BorderStyle;
    let unstyled = |s: BorderStyle| matches!(s, BorderStyle::None | BorderStyle::Hidden);
    if unstyled(style.border_top_style) {
        style.border_top_width = 0.0;
    }
    if unstyled(style.border_right_style) {
        style.border_right_width = 0.0;
    }
    if unstyled(style.border_bottom_style) {
        style.border_bottom_width = 0.0;
    }
    if unstyled(style.border_left_style) {
        style.border_left_width = 0.0;
    }
}
