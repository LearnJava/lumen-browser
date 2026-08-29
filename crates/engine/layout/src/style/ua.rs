//! UA-таблица стилей и её правки под элементы: дефолтный `display` (HTML
//! rendering §15.3), UA-хинты шрифта/цвета/выключки для конкретных тегов,
//! стили формы (`<input>`/`<button>`/`<select>`/…), `<hr>`/`<body>`/заголовки,
//! `<dialog>`, `<td>`/`<th>` padding, `[inert]`.
//!
//! Перенесено батчем SPLIT-ST11 из `crates/engine/layout/src/style.rs`
//! (анкер `fn default_display`) без правок тел: изменены только видимость
//! item-ов (`pub(in crate::style)`, кроме `ua_form_element_colors`, которая
//! была `pub` и осталась `pub`) и пути импортов.

use lumen_dom::{Document, NodeData, NodeId};

use crate::style::{
    BorderStyle, Color, ComputedStyle, CssColor, Display, FontStyle, FontWeight, Length,
    LengthOrAuto, PointerEvents, VerticalAlign, WhiteSpace,
};

// ──────────────── default display / declarations ────────────────

pub(in crate::style) fn default_display(doc: &Document, node: NodeId) -> Display {
    let NodeData::Element { name, .. } = &doc.get(node).data else {
        return Display::Block;
    };
    match name.local.as_str() {
        // <head> и его метаданные никогда не рендерятся как видимый контент.
        // `<source>` и `<track>` — child-кандидаты `<picture>` / `<video>` /
        // `<audio>`; реальное визуальное представление даёт inner `<img>`
        // (резолвится `pick_picture_source`) или сам media-элемент. Сами
        // эти теги в DOM есть, но layout-бокса не порождают.
        "head" | "title" | "style" | "script" | "meta" | "link" | "base" | "noscript"
        | "source" | "track" => Display::None,
        // Inline-уровневые элементы. Phase 0: пока трактуем как block — текст
        // внутри `<a>`/`<span>` будет на своей строке. Это известное ограничение.
        "a" | "span" | "b" | "i" | "em" | "strong" | "code" | "small" | "sub" | "sup"
        | "label" | "abbr" | "cite" | "q" | "mark" | "u"
        // HTML §15.3.7: <del>, <ins>, <s> — flow content, UA display = inline.
        | "del" | "ins" | "s" => Display::Inline,
        // HTML rendering §15.3.1 — `<img>` is inline-level replaced content, so
        // it shares the line box with the text around it (icon in a button, logo
        // next to a title, avatar in a comment). It never becomes an `InlineRun`
        // segment — a segment has no height of its own (BUG-728) — but
        // `is_atomic_inline_level` picks it up as an atomic inline-level box and
        // it flows inside `InlineBlockRow` beside text and `inline-block`
        // siblings (IFC-2). Author `display:` overrides win through the cascade.
        "img" => Display::Inline,
        // CSS 2.1 table model — UA default display values per HTML spec.
        "table" => Display::Table,
        "caption" => Display::TableCaption,
        "colgroup" => Display::TableColumnGroup,
        "col" => Display::TableColumn,
        "thead" => Display::TableHeaderGroup,
        "tbody" => Display::TableRowGroup,
        "tfoot" => Display::TableFooterGroup,
        "tr" => Display::TableRow,
        "td" | "th" => Display::TableCell,
        // CSS 2.1 — list-item UA default.
        "li" => Display::ListItem,
        // HTML rendering §15.3.1 / §15.5 — form controls are `inline-block`
        // by default, so they flow horizontally with surrounding inline content
        // (text labels, sibling controls) instead of each taking its own line.
        // Author `display:` overrides win through the normal cascade.
        "input" | "button" | "select" | "selectlist" | "textarea" | "meter" | "progress" => {
            Display::InlineBlock
        }
        // HTML rendering §15.5.3 — `<option>` is not rendered in the document
        // flow of a closed `<select>`; the selected label is read straight from
        // the DOM (`collect_select_label`) and painted by the select widget.
        // `display:none` suppresses the painted option text (which otherwise
        // leaks below/over the control) while still generating a (non-painted)
        // box, so `:disabled`/`:checked` selector matching on options keeps
        // working. `<optgroup>` stays in flow (it has no rendered text of its
        // own — only an attribute label — and must recurse so descendant option
        // styles are still computed).
        "option" => Display::None,
        _ => Display::Block,
    }
}
/// HTML5 §14.3.3 — UA white-space for specific elements.
/// Returns `Some` only for elements that override the inherited value.
pub(in crate::style) fn ua_white_space(doc: &Document, node: NodeId) -> Option<WhiteSpace> {
    let NodeData::Element { name, .. } = &doc.get(node).data else {
        return None;
    };
    match name.local.as_str() {
        // HTML5 UA stylesheet: pre, listing, xmp, plaintext — white-space: pre
        "pre" | "listing" | "xmp" | "plaintext" => Some(WhiteSpace::Pre),
        // textarea — white-space: pre-wrap (per HTML5 rendering spec)
        "textarea" => Some(WhiteSpace::PreWrap),
        _ => None,
    }
}
/// Эмулирует UA stylesheet для font-style: HTML §15.3.3 рекомендует italic
/// для `<em>` / `<i>` / `<cite>` / `<dfn>` / `<address>` / `<var>`. Возвращает
/// `Some(Italic)` для них, `None` для остальных (= наследовать как обычно).
pub(in crate::style) fn ua_font_style(doc: &Document, node: NodeId) -> Option<FontStyle> {
    let NodeData::Element { name, .. } = &doc.get(node).data else {
        return None;
    };
    match name.local.as_str() {
        "em" | "i" | "cite" | "dfn" | "address" | "var" => Some(FontStyle::Italic),
        _ => None,
    }
}
/// Эмулирует UA stylesheet для `font-family`: HTML §15.3.2 задаёт
/// `font-family: monospace` для `<pre>` / `<code>` / `<kbd>` / `<samp>` /
/// `<tt>` (плюс исторические `<listing>` / `<xmp>` / `<plaintext>`,
/// которые уже получают `white-space: pre` рядом).
///
/// Возвращает `Some(["monospace"])` для них, `None` для остальных
/// (= наследовать как обычно). Generic-имя резолвится в конкретный системный
/// шрифт на этапе рендера/измерения (BUG-128); до этого моноширинные элементы
/// рисовались пропорциональным Inter-ом.
pub(in crate::style) fn ua_font_family(doc: &Document, node: NodeId) -> Option<Vec<String>> {
    let NodeData::Element { name, .. } = &doc.get(node).data else {
        return None;
    };
    match name.local.as_str() {
        "pre" | "code" | "kbd" | "samp" | "tt" | "listing" | "xmp" | "plaintext" => {
            Some(vec!["monospace".to_string()])
        }
        _ => None,
    }
}
/// UA stylesheet: `text-decoration` для семантических HTML-элементов
/// (HTML5 §15.3.7 + §15.3.3).
///
/// - `<del>`, `<s>` → `line-through`
/// - `<ins>`, `<u>` → `underline`
/// - `<a>` (с атрибутом `href`) → `underline`
///
/// Устанавливается ДО CSS-каскада, поэтому любое author-правило перекроет.
/// `<u>` уже в списке inline-элементов — эта функция добавляет ему decoration.
pub(in crate::style) fn apply_ua_text_decoration(doc: &Document, node: NodeId, style: &mut ComputedStyle) {
    let NodeData::Element { name, .. } = &doc.get(node).data else {
        return;
    };
    match name.local.as_str() {
        "del" | "s" => {
            style.text_decoration_line.line_through = true;
        }
        "ins" | "u" => {
            style.text_decoration_line.underline = true;
        }
        "a" if doc.get(node).get_attr("href").is_some() => {
            style.text_decoration_line.underline = true;
        }
        _ => {}
    }
}
/// UA stylesheet: `color` для `<a href="…">`.
/// HTML5 §15.3.3: unvisited links → `color: -webkit-link` (обычно #0000ee).
/// Phase 0 не поддерживает `:visited` — все `<a>` получают link-color.
/// Возвращает `Some(color)` только если у элемента есть `href` атрибут
/// (якорные `<a>` без `href` не являются гиперссылками).
pub(in crate::style) fn ua_link_color(doc: &Document, node: NodeId) -> Option<Color> {
    let NodeData::Element { name, .. } = &doc.get(node).data else {
        return None;
    };
    if name.local.as_str() == "a" && doc.get(node).get_attr("href").is_some() {
        Some(Color { r: 0, g: 0, b: 238, a: 255 }) // #0000ee
    } else {
        None
    }
}
/// UA stylesheet: масштаб font-size для `<small>`, `<sub>`, `<sup>`.
/// HTML5 §15.3.3: font-size: smaller (≈ 0.83× родительского).
/// Возвращает `Some(factor)` — multiplier к `parent_font_size`.
pub(in crate::style) fn ua_font_size_factor(doc: &Document, node: NodeId) -> Option<f32> {
    let NodeData::Element { name, .. } = &doc.get(node).data else {
        return None;
    };
    match name.local.as_str() {
        "small" | "sub" | "sup" => Some(0.83),
        _ => None,
    }
}
/// UA stylesheet: `vertical-align` для `<sub>` и `<sup>`.
/// HTML5 §15.3.3: sub → Sub, sup → Super.
pub(in crate::style) fn ua_vertical_align(doc: &Document, node: NodeId) -> Option<VerticalAlign> {
    let NodeData::Element { name, .. } = &doc.get(node).data else {
        return None;
    };
    match name.local.as_str() {
        "sub" => Some(VerticalAlign::Sub),
        "sup" => Some(VerticalAlign::Super),
        _ => None,
    }
}
/// UA stylesheet для font-weight: `<b>`, `<strong>`, `<th>`, `<h1>`–`<h6>`
/// получают bold по умолчанию (HTML §15.3.3).
pub(in crate::style) fn ua_font_weight(doc: &Document, node: NodeId) -> Option<FontWeight> {
    let NodeData::Element { name, .. } = &doc.get(node).data else {
        return None;
    };
    match name.local.as_str() {
        "b" | "strong" | "th" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
            Some(FontWeight::BOLD)
        }
        _ => None,
    }
}
/// UA stylesheet для `<hr>` (HTML5 §15.3.7 / Rendering §14.6).
///
/// Браузеры рендерят `<hr>` как 1px-линию через border-top с авто-маргинами.
/// Author CSS может перекрыть любое из этих значений.
pub(in crate::style) fn apply_ua_hr_style(doc: &Document, node: NodeId, style: &mut ComputedStyle) {
    let NodeData::Element { name, .. } = &doc.get(node).data else {
        return;
    };
    if name.local.as_str() != "hr" {
        return;
    }
    style.border_top_width = 1.0;
    style.border_top_style = BorderStyle::Solid;
    style.border_top_color = CssColor::Rgba(Color { r: 118, g: 118, b: 118, a: 255 });
    style.margin_top = LengthOrAuto::Length(Length::Em(0.5));
    style.margin_bottom = LengthOrAuto::Length(Length::Em(0.5));
    style.margin_left = LengthOrAuto::Auto;
    style.margin_right = LengthOrAuto::Auto;
}
/// UA stylesheet для `<body>` (HTML Rendering §14.3.3): `body { margin: 8px }`.
///
/// Без этого правила `<body>` прижимается вплотную к краю viewport, и весь
/// контент в нормальном потоке сдвинут на 8px относительно настоящих браузеров.
/// Применяется ДО CSS-каскада, поэтому author `body { margin: 0 }` или
/// `* { margin: 0 }` перекрывает его (как в большинстве graphic-тестов с reset).
///
/// BUG-204: страницы anchor-positioning (тесты 85–89) без CSS-reset расходились
/// с Edge на ~2% — Edge сдвигал `.__f`-рамку на 8px (body margin), Lumen рисовал
/// её вплотную к краю.
pub(in crate::style) fn apply_ua_body_margin(doc: &Document, node: NodeId, style: &mut ComputedStyle) {
    let NodeData::Element { name, .. } = &doc.get(node).data else {
        return;
    };
    if name.local.as_str() != "body" {
        return;
    }
    style.margin_top = LengthOrAuto::Length(Length::Px(8.0));
    style.margin_right = LengthOrAuto::Length(Length::Px(8.0));
    style.margin_bottom = LengthOrAuto::Length(Length::Px(8.0));
    style.margin_left = LengthOrAuto::Length(Length::Px(8.0));
}
/// UA stylesheet для `<h1>`–`<h6>` (HTML Rendering §15.3.3 «Sections and headings»).
///
/// Браузеры задают заголовкам увеличенный `font-size` (em относительно
/// родителя) и вертикальные `margin` (em относительно собственного
/// computed font-size). `font-weight: bold` уже выставляется `ua_font_weight`.
///
/// `font_size` пишется как computed px (`inherited.font_size * factor`) — так же,
/// как `ua_font_size_factor` для `<small>`/`<sub>`/`<sup>`; author `font-size`
/// перекроет его в font-size pre-pass. Маргины задаются как `Em`, поэтому
/// резолвятся против финального font-size заголовка на этапе layout; author CSS
/// перекроет их в main-pass каскада.
///
/// Значения (font-size factor, vertical margin em):
/// h1 2.0/0.67, h2 1.5/0.83, h3 1.17/1.0, h4 1.0/1.33, h5 0.83/1.67, h6 0.67/2.33.
pub(in crate::style) fn apply_ua_heading_style(doc: &Document, node: NodeId, inherited: &ComputedStyle, style: &mut ComputedStyle) {
    let NodeData::Element { name, .. } = &doc.get(node).data else {
        return;
    };
    let (size_factor, margin_em) = match name.local.as_str() {
        "h1" => (2.0, 0.67),
        "h2" => (1.5, 0.83),
        "h3" => (1.17, 1.0),
        "h4" => (1.0, 1.33),
        "h5" => (0.83, 1.67),
        "h6" => (0.67, 2.33),
        _ => return,
    };
    style.font_size = inherited.font_size * size_factor;
    style.margin_top = LengthOrAuto::Length(Length::Em(margin_em));
    style.margin_bottom = LengthOrAuto::Length(Length::Em(margin_em));
}
/// UA stylesheet для HTML form controls (HTML5 §15.5 «Rendering»).
///
/// Returns UA colors `(border, background, foreground)` for form controls.
///
/// Approximates CSS Color 4 system-color keywords (`ButtonFace`, `Field`,
/// `ButtonText`, `FieldText`) without a full system-color implementation.
/// Used by `apply_ua_form_controls` to theme controls for light/dark mode.
///
/// - Light: border #767676 / bg white (inputs) or #efefef (button) / fg black
/// - Dark:  border #616161 / bg #1e1e1e (inputs) or #3a3a3c (button) / fg white
///
/// `// CSS: color-scheme` — P4 wires this to `ComputedStyle.color_scheme`
/// for full system-color keyword support.
pub fn ua_form_element_colors(tag: &str, dark_mode: bool) -> (CssColor, CssColor, Color) {
    if dark_mode {
        let border = CssColor::Rgba(Color { r: 97, g: 97, b: 97, a: 255 });
        let fg = Color { r: 255, g: 255, b: 255, a: 255 };
        let bg = if tag == "button" {
            CssColor::Rgba(Color { r: 58, g: 58, b: 60, a: 255 })
        } else {
            CssColor::Rgba(Color { r: 30, g: 30, b: 30, a: 255 })
        };
        (border, bg, fg)
    } else {
        let border = CssColor::Rgba(Color { r: 118, g: 118, b: 118, a: 255 });
        let fg = Color { r: 0, g: 0, b: 0, a: 255 };
        let bg = if tag == "button" {
            CssColor::Rgba(Color { r: 239, g: 239, b: 239, a: 255 })
        } else {
            CssColor::Rgba(Color { r: 255, g: 255, b: 255, a: 255 })
        };
        (border, bg, fg)
    }
}
/// Применяется ДО CSS-каскада — любой author-rule перекрывает.
/// - `<input type=hidden>` → `display: none`
/// - `<input type=checkbox|radio>` → 13×13 px
/// - `<input>` (остальные) → 174×21 px
/// - `<button>` → height 21 px
/// - `<textarea>` → 200×48 px
/// - `<select>` → height 21 px
/// - `<progress>` → 300×16 px
/// - `<meter>` → 300×16 px
/// - Все кроме hidden → border, background, color по `ua_form_element_colors`
pub(in crate::style) fn apply_ua_form_controls(doc: &Document, node: NodeId, style: &mut ComputedStyle, dark_mode: bool) {
    let NodeData::Element { name, .. } = &doc.get(node).data else { return; };
    let tag = name.local.as_str();
    match tag {
        "input" => {
            let ty = doc
                .get(node)
                .get_attr("type")
                .map(|s| s.to_ascii_lowercase())
                .unwrap_or_else(|| "text".to_string());
            match ty.trim() {
                "hidden" => {
                    style.display = Display::None;
                    return;
                }
                "checkbox" | "radio" => {
                    style.width = Some(Length::Px(13.0));
                    style.height = Some(Length::Px(13.0));
                }
                "range" => {
                    style.width = Some(Length::Px(129.0));
                    style.height = Some(Length::Px(20.0));
                    // Range input has no visible border — track/thumb are drawn by paint.
                    return;
                }
                _ => {
                    style.width = Some(Length::Px(174.0));
                    style.height = Some(Length::Px(21.0));
                }
            }
        }
        "button" => {
            style.height = Some(Length::Px(21.0));
        }
        "textarea" => {
            style.width = Some(Length::Px(200.0));
            style.height = Some(Length::Px(48.0));
        }
        "select" => {
            style.height = Some(Length::Px(21.0));
        }
        "progress" | "meter" => {
            style.width = Some(Length::Px(300.0));
            style.height = Some(Length::Px(16.0));
        }
        _ => return,
    }
    let (border, bg, fg) = ua_form_element_colors(tag, dark_mode);
    style.border_top_width = 1.0;
    style.border_right_width = 1.0;
    style.border_bottom_width = 1.0;
    style.border_left_width = 1.0;
    style.border_top_style = BorderStyle::Solid;
    style.border_right_style = BorderStyle::Solid;
    style.border_bottom_style = BorderStyle::Solid;
    style.border_left_style = BorderStyle::Solid;
    style.border_top_color = border;
    style.border_right_color = border;
    style.border_bottom_color = border;
    style.border_left_color = border;
    style.background_color = Some(bg);
    style.color = fg;
}
/// CSS Basic UI L4 §4.4 — post-cascade pass: when `field-sizing: content` was set
/// by the author stylesheet, clears any UA-supplied `width`/`height` on text-entry
/// controls so that `lay_out` will call `field_sizing_content_intrinsic` instead.
///
/// Must run AFTER the CSS cascade so that author `field-sizing: content` is final.
pub(in crate::style) fn apply_ua_form_controls_field_sizing_clear(doc: &Document, node: NodeId, style: &mut ComputedStyle) {
    let NodeData::Element { name, .. } = &doc.get(node).data else { return; };
    match name.local.as_str() {
        "input" => {
            let ty = doc
                .get(node)
                .get_attr("type")
                .map(|s| s.to_ascii_lowercase())
                .unwrap_or_else(|| "text".to_string());
            // Only text-entry types are eligible; checkbox/radio/range use fixed sizes.
            match ty.trim() {
                "checkbox" | "radio" | "range" | "hidden" => {}
                _ => {
                    style.width = None;
                    style.height = None;
                }
            }
        }
        "textarea" => {
            style.width = None;
            style.height = None;
        }
        _ => {}
    }
}
/// CSS Basic UI L4 §5 — strips UA-default styling (border, padding, background)
/// from a form control under `appearance: none`.
///
/// Called *before* the author cascade (gated on the pre-scanned cascade-winning
/// `appearance` value, see `compute_style`) so author-specified
/// border/background/padding declarations apply on top of the cleared UA
/// defaults. Running this *after* the cascade (the pre-BUG-211 behaviour)
/// clobbered author values, leaving content-sized fields with width-0 borders
/// and a transparent background.
///
/// Applies to: <input>, <button>, <select>, <textarea>, <progress>, <meter>.
pub(in crate::style) fn strip_ua_appearance_box_styling(doc: &Document, node: NodeId, style: &mut ComputedStyle) {
    let NodeData::Element { name, .. } = &doc.get(node).data else { return; };
    match name.local.as_str() {
        "input" | "button" | "select" | "textarea" | "progress" | "meter" => {
            // Remove UA border
            style.border_top_width = 0.0;
            style.border_right_width = 0.0;
            style.border_bottom_width = 0.0;
            style.border_left_width = 0.0;
            // Remove UA padding
            style.padding_top = Length::Px(0.0);
            style.padding_right = Length::Px(0.0);
            style.padding_bottom = Length::Px(0.0);
            style.padding_left = Length::Px(0.0);
            // Remove UA background (fully transparent)
            style.background_color = Some(CssColor::Rgba(Color { r: 0, g: 0, b: 0, a: 0 }));
        }
        _ => {}
    }
}
/// UA stylesheet: `<dialog>` without the `open` attribute → `display: none`.
/// HTML5 §15.3.9: "dialog:not([open]) { display: none; }"
pub(in crate::style) fn apply_ua_dialog_display(doc: &Document, node: NodeId, style: &mut ComputedStyle) {
    let NodeData::Element { name, .. } = &doc.get(node).data else { return; };
    if name.local.as_str() == "dialog" && doc.get(node).get_attr("open").is_none() {
        style.display = Display::None;
    }
}
/// UA stylesheet (HTML Rendering §15.3.8): `td, th { padding: 1px }`.
///
/// Table cells get a default 1px padding on all four sides. The legacy
/// `cellpadding` attribute on the nearest ancestor `<table>` overrides this for
/// every cell (HTML §14.3.9.1): a non-negative numeric value sets the padding,
/// so `cellpadding="0"` (ubiquitous in legacy layout tables) restores zero.
/// Applied during the pre-cascade UA phase so author `padding` declarations win.
pub(in crate::style) fn apply_ua_table_cell_padding(doc: &Document, node: NodeId, style: &mut ComputedStyle) {
    let node_ref = doc.get(node);
    let NodeData::Element { name, .. } = &node_ref.data else { return; };
    if !matches!(name.local.as_str(), "td" | "th") {
        return;
    }
    // Default 1px; an ancestor <table cellpadding=N> overrides it.
    let mut pad = 1.0_f32;
    let mut cur = node_ref.parent;
    while let Some(p) = cur {
        let p_ref = doc.get(p);
        if let NodeData::Element { name: pname, .. } = &p_ref.data
            && pname.local.as_str() == "table"
        {
            if let Some(v) = p_ref.get_attr("cellpadding")
                && let Ok(n) = v.trim().parse::<f32>()
                && n >= 0.0
            {
                pad = n;
            }
            break;
        }
        cur = p_ref.parent;
    }
    style.padding_top = Length::Px(pad);
    style.padding_right = Length::Px(pad);
    style.padding_bottom = Length::Px(pad);
    style.padding_left = Length::Px(pad);
}
/// UA stylesheet (HTML Rendering §15.4.2): `[inert] { pointer-events: none; }`.
///
/// An element carrying the `inert` boolean attribute — and, because inertness is
/// inherited down the DOM tree, every descendant of such an element — is made
/// non-interactive. The UA origin sets `pointer-events: none` so that
/// `ComputedStyle.pointer_events` reflects inertness (e.g. for `getComputedStyle`
/// and cursor resolution), complementing the layout-level hit-test filter in
/// `collect_clickable_elements` (lumen-layout `lib.rs`, see `// CSS: inert`).
///
/// Applied during the pre-cascade UA phase, so an author `pointer-events`
/// declaration overrides it (UA origin has the lowest cascade priority).
/// [`inert::is_inert`] walks the ancestor chain, so a node nested inside an
/// inert subtree is matched even when it carries no `inert` attribute itself.
pub(in crate::style) fn apply_ua_inert(doc: &Document, node: NodeId, style: &mut ComputedStyle) {
    if crate::inert::is_inert(doc, node) {
        style.pointer_events = PointerEvents::None;
    }
}
