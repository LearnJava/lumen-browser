//! HTML presentational hints (HTML5 §15.3): `width`/`height`/`hspace`/`vspace`/
//! `border` для `<img>`/`<video>`/`<iframe>`, `bgcolor`/`background`/`text`/
//! `color`/`link` для legacy-атрибутов, SVG presentation attributes,
//! `<font>`/`align`/`<td width>`, разбор HTML-length и legacy-color значений
//! (HTML5 §2.4.6 «rules for parsing a legacy color value»).
//!
//! Перенесено батчем SPLIT-ST11 из `crates/engine/layout/src/style.rs`
//! (анкер `fn apply_image_presentational_hints`) без правок тел: изменены
//! только видимость item-ов (`pub(in crate::style)`) и пути импортов.

use lumen_core::geom::Size;
use lumen_css_parser::Declaration;
use lumen_dom::{Document, DocumentMode, NodeData, NodeId};

use crate::style::{
    apply_declaration, named_color, parse_font_family, BackgroundImage, BackgroundLayer,
    BorderStyle, Color, ComputedStyle, CssColor, FontWeight, Length, LengthOrAuto, TextAlign,
};

/// Применяет HTML presentational hints для `<img>`, `<video>`, `<iframe>`:
/// `width`/`height`, `hspace`/`vspace` (→ margin), `border` для `<img>`.
/// HTML5 §15.3.9. Author CSS поверх — выигрывает.
pub(in crate::style) fn apply_image_presentational_hints(doc: &Document, node: NodeId, style: &mut ComputedStyle) {
    let NodeData::Element { name, .. } = &doc.get(node).data else {
        return;
    };
    let is_img = name.local == "img";
    let is_video = name.local == "video";
    let is_iframe = name.local == "iframe";
    if !is_img && !is_video && !is_iframe {
        return;
    }
    let node_ref = doc.get(node);
    if let Some(w) = node_ref.get_attr("width").and_then(parse_html_dimension) {
        style.width = Some(Length::Px(w));
    }
    if let Some(h) = node_ref.get_attr("height").and_then(parse_html_dimension) {
        style.height = Some(Length::Px(h));
    }
    // hspace/vspace/border are <img>-only presentational attributes (HTML5 §15.3.9).
    if is_img {
        if let Some(h) = node_ref.get_attr("hspace").and_then(parse_html_dimension) {
            style.margin_left = LengthOrAuto::Length(Length::Px(h));
            style.margin_right = LengthOrAuto::Length(Length::Px(h));
        }
        if let Some(v) = node_ref.get_attr("vspace").and_then(parse_html_dimension) {
            style.margin_top = LengthOrAuto::Length(Length::Px(v));
            style.margin_bottom = LengthOrAuto::Length(Length::Px(v));
        }
        if let Some(b) = node_ref.get_attr("border").and_then(parse_html_dimension) {
            style.border_top_width = b;
            style.border_right_width = b;
            style.border_bottom_width = b;
            style.border_left_width = b;
            if b > 0.0 {
                style.border_top_style = BorderStyle::Solid;
                style.border_right_style = BorderStyle::Solid;
                style.border_bottom_style = BorderStyle::Solid;
                style.border_left_style = BorderStyle::Solid;
            }
        }
    }
}
/// SVG 2 §6.4 — SVG presentation attributes. Geometry and paint properties on
/// SVG elements may be given as plain XML attributes (e.g. `<path fill="none"
/// stroke="#e94560" stroke-width="8">`) instead of CSS. Each maps onto the
/// corresponding CSS property, but with the **lowest author-origin priority**:
/// any matching CSS rule (stylesheet selector or inline `style=""`) overrides it.
///
/// We therefore apply them *before* the matched-declaration cascade loop, reusing
/// `apply_declaration` for parsing so the attribute and the CSS form share one
/// code path. Gated by SVG tag name so HTML attributes coincidentally named
/// `fill`/`stroke`/`color` on non-SVG elements are not reinterpreted as paint.
#[allow(clippy::too_many_arguments)]
pub(in crate::style) fn apply_svg_presentational_hints(
    doc: &Document,
    node: NodeId,
    style: &mut ComputedStyle,
    em_basis: f32,
    viewport: Size,
    parent_weight: FontWeight,
    inherited: &ComputedStyle,
    is_quirks: bool,
) {
    let NodeData::Element { name, .. } = &doc.get(node).data else {
        return;
    };
    if !is_svg_presentational_element(name.local.as_ref()) {
        return;
    }
    // Presentation attributes recognised by `apply_declaration`
    // (SVG §11 paint + §13 stroke geometry, plus `color` for `currentColor`).
    const ATTRS: &[&str] = &[
        "fill",
        "fill-opacity",
        "fill-rule",
        "clip-rule",
        "stroke",
        "stroke-opacity",
        "stroke-width",
        "stroke-linecap",
        "stroke-linejoin",
        "stroke-miterlimit",
        "stroke-dasharray",
        "stroke-dashoffset",
        "text-anchor",
        "dominant-baseline",
        "baseline-shift",
        "color",
        "opacity",
    ];
    let node_ref = doc.get(node);
    for &attr in ATTRS {
        let Some(val) = node_ref.get_attr(attr) else { continue };
        if val.trim().is_empty() {
            continue;
        }
        let decl = Declaration {
            property: attr.to_string(),
            value: val.to_string(),
            important: false,
        };
        apply_declaration(style, &decl, em_basis, viewport, parent_weight, inherited, inherited, is_quirks, false);
    }
}
/// True for SVG element local names that accept SVG presentation attributes.
/// Covers the shapes/containers Lumen lays out plus text elements.
pub(in crate::style) fn is_svg_presentational_element(local: &str) -> bool {
    matches!(
        local,
        "svg" | "g" | "rect" | "circle" | "ellipse" | "line" | "path"
            | "polygon" | "polyline" | "text" | "tspan" | "textPath" | "use"
    )
}
/// HTML5 §15: `bgcolor` атрибут на `<body>` / table-related элементах
/// мапается на `background-color` (presentational hint). Парсится через
/// HTML5 §2.4.6 «rules for parsing a legacy color value». Любое author-CSS
/// правило в каскаде ниже перекроет hint — так и устроена presentational
/// hint конструкция.
///
/// Список тегов взят из HTML5 §15.3.6 (`<body>`) и §15.3.8 (table-tree).
/// Phase 0 ещё не делает табличный layout — но bgcolor попадает в
/// `style.background_color` всё равно, чтобы при появлении table-layout
/// рендеринг сразу работал.
pub(in crate::style) fn apply_bgcolor_presentational_hint(doc: &Document, node: NodeId, style: &mut ComputedStyle) {
    let NodeData::Element { name, .. } = &doc.get(node).data else {
        return;
    };
    let tag = name.local.as_str();
    if !matches!(
        tag,
        "body" | "table" | "thead" | "tbody" | "tfoot" | "tr" | "td" | "th"
    ) {
        return;
    }
    let node_ref = doc.get(node);
    if let Some(val) = node_ref.get_attr("bgcolor")
        && let Some(c) = parse_legacy_color_html_attr(val)
    {
        style.background_color = Some(CssColor::Rgba(c));
    }
}
/// HTML LS §15.3.8 «Tables»: `background` атрибут на `<body>` / table-tree
/// элементах мапается на `background-image` (BUG-603 point 2). Тот же tag-set,
/// что и у [`apply_bgcolor_presentational_hint`]. В отличие от `bgcolor`,
/// значение не резолвится в абсолютный URL здесь — как и обычный CSS
/// `background: url(...)`, сырая строка хранится в `BackgroundImage::Url` и
/// резолвится относительно document base URL на paint/fetch стороне (см.
/// использование `BackgroundImage::Url` в CSS-парсинге фона выше — там тоже
/// хранится сырой текст, не резолвленный путь).
pub(in crate::style) fn apply_background_image_presentational_hint(doc: &Document, node: NodeId, style: &mut ComputedStyle) {
    let NodeData::Element { name, .. } = &doc.get(node).data else {
        return;
    };
    let tag = name.local.as_str();
    if !matches!(
        tag,
        "body" | "table" | "thead" | "tbody" | "tfoot" | "tr" | "td" | "th"
    ) {
        return;
    }
    let node_ref = doc.get(node);
    if let Some(val) = node_ref.get_attr("background") {
        let val = val.trim();
        if !val.is_empty() {
            if style.background_layers.is_empty() {
                style.background_layers.push(BackgroundLayer::default());
            }
            style.background_layers[0].image = BackgroundImage::Url(val.to_string());
        }
    }
}
/// HTML LS §15.3.8 «Tables»: `bordercolor` атрибут на table-tree элементах
/// мапается на все четыре `border-*-color` (BUG-603 point 2). Парсится тем же
/// legacy-парсером, что и `bgcolor`/`text`/`font color`. Не включает `<body>`
/// (spec ограничивает `bordercolor` собственно табличными элементами).
pub(in crate::style) fn apply_bordercolor_presentational_hint(doc: &Document, node: NodeId, style: &mut ComputedStyle) {
    let NodeData::Element { name, .. } = &doc.get(node).data else {
        return;
    };
    let tag = name.local.as_str();
    if !matches!(
        tag,
        "table" | "thead" | "tbody" | "tfoot" | "tr" | "td" | "th"
    ) {
        return;
    }
    let node_ref = doc.get(node);
    if let Some(val) = node_ref.get_attr("bordercolor")
        && let Some(c) = parse_legacy_color_html_attr(val)
    {
        let color = CssColor::Rgba(c);
        style.border_top_color = color;
        style.border_right_color = color;
        style.border_bottom_color = color;
        style.border_left_color = color;
    }
}
/// HTML LS §15.3.8 «Tables»: `cellspacing` атрибут на `<table>` мапается на
/// `border-spacing` (BUG-603 point 2) — один legacy-атрибут задаёт оба
/// компонента (horizontal и vertical) одинаково, симметрично `cellpadding`→
/// `padding` в [`apply_ua_table_cell_padding`]. Unlike `cellpadding`, this
/// applies directly to the `<table>` element itself (`border-spacing` is not
/// something a `<td>`/`<tr>` reads), not via an ancestor walk.
pub(in crate::style) fn apply_cellspacing_presentational_hint(doc: &Document, node: NodeId, style: &mut ComputedStyle) {
    let NodeData::Element { name, .. } = &doc.get(node).data else {
        return;
    };
    if name.local.as_str() != "table" {
        return;
    }
    let node_ref = doc.get(node);
    if let Some(val) = node_ref.get_attr("cellspacing")
        && let Ok(n) = val.trim().parse::<f32>()
        && n >= 0.0
    {
        style.border_spacing_h = n;
        style.border_spacing_v = n;
    }
}
/// HTML5 §15.3.6 «The page» (для `<body text>`) + §15.3.2 «Phrasing
/// content» (для `<font color>`): мапает legacy-атрибуты на CSS `color`.
///
/// - `<body text="…">` → `body.color`. Через CSS-наследование цвет
///   распространяется на всех потомков, у которых нет явного `color`.
/// - `<font color="…">` → элементный `color`. Атрибут применим к любому
///   элементу с именем `font`, в т.ч. внутри других элементов.
///
/// `<body link/vlink/alink>` отложены: hyperlink coloring требует UA
/// stylesheet с descendant-селектором (`body :link { color: … }`), а в
/// Phase 0 без visited/active runtime два из трёх атрибутов всё равно
/// были бы no-op.
///
/// Парсинг — `parse_legacy_color_html_attr` (HTML5 §2.4.6). Hint
/// применяется ДО CSS-каскада, поэтому любое author-CSS правило
/// перекроет атрибут.
pub(in crate::style) fn apply_text_color_presentational_hint(
    doc: &Document,
    node: NodeId,
    style: &mut ComputedStyle,
) {
    let NodeData::Element { name, .. } = &doc.get(node).data else {
        return;
    };
    let tag = name.local.as_str();
    let node_ref = doc.get(node);
    let attr_name = match tag {
        "body" => "text",
        "font" => "color",
        _ => return,
    };
    if let Some(val) = node_ref.get_attr(attr_name)
        && let Some(c) = parse_legacy_color_html_attr(val)
    {
        style.color = c;
    }
}
/// HTML5 §15.3.2: `<font size="N">` → абсолютный font-size; `<font face="…">` → font-family.
///
/// Значения `size` 1–7 отображаются на CSS absolute-size keywords (medium = 16px):
/// 1→10px 2→13px 3→16px 4→18px 5→24px 6→32px 7→48px.
/// Относительные (`+2`, `-1`) прибавляются к базе 3, затем клэмпируются в [1,7].
/// Hint применяется ДО CSS-каскада, поэтому author font-size/font-family перекроет.
pub(in crate::style) fn apply_font_element_presentational_hints(
    doc: &Document,
    node: NodeId,
    style: &mut ComputedStyle,
) {
    let NodeData::Element { name, .. } = &doc.get(node).data else {
        return;
    };
    if name.local.as_str() != "font" {
        return;
    }
    let node_ref = doc.get(node);

    // `size` attribute → font-size.
    if let Some(val) = node_ref.get_attr("size") {
        let val = val.trim();
        let size_num: Option<i32> = if let Some(rel) = val.strip_prefix('+') {
            rel.parse::<i32>().ok().map(|d| 3 + d)
        } else if let Some(rel) = val.strip_prefix('-') {
            rel.parse::<i32>().ok().map(|d| 3 - d)
        } else {
            val.parse::<i32>().ok()
        };
        if let Some(n) = size_num {
            // Clamp to [1, 7] then map to absolute px per HTML5 §15.3.2.
            let px: f32 = match n.clamp(1, 7) {
                1 => 10.0,
                2 => 13.0,
                3 => 16.0,
                4 => 18.0,
                5 => 24.0,
                6 => 32.0,
                _ => 48.0, // 7
            };
            style.font_size = px;
        }
    }

    // `face` attribute → font-family.
    if let Some(val) = node_ref.get_attr("face") {
        let families = parse_font_family(val);
        if !families.is_empty() {
            style.font_family = families;
        }
    }
}
/// HTML5 §15.3.3: атрибут `align` на блочных элементах → CSS `text-align`.
///
/// Применяется к: div, p, h1–h6, blockquote, address, dt, dd, caption.
/// Значения: left→Left, right→Right, center/middle→Center, justify→Justify.
/// Hint применяется ДО CSS-каскада, author text-align перекроет.
pub(in crate::style) fn apply_align_presentational_hint(doc: &Document, node: NodeId, style: &mut ComputedStyle) {
    let NodeData::Element { name, .. } = &doc.get(node).data else {
        return;
    };
    if !matches!(
        name.local.as_str(),
        "div" | "p"
            | "h1"
            | "h2"
            | "h3"
            | "h4"
            | "h5"
            | "h6"
            | "blockquote"
            | "address"
            | "dt"
            | "dd"
            | "caption"
    ) {
        return;
    }
    let node_ref = doc.get(node);
    if let Some(val) = node_ref.get_attr("align") {
        let ta = match val.trim().to_ascii_lowercase().as_str() {
            "left" => TextAlign::Left,
            "right" => TextAlign::Right,
            "center" | "middle" => TextAlign::Center,
            _ => return,
        };
        style.text_align = ta;
    }
}
/// CSS Quirks Mode §4.1 + HTML5 §14.3.9: `width`/`height` presentational
/// hints для ячеек таблицы (`<td>`, `<th>`) и самого `<table>`.
///
/// `<table width="N">` → `width: Npx` (оба режима).
/// `<td width="N">` / `<th width="N">`:
///   - Standards mode → `width: Npx`
///   - Quirks mode → `min-width: Npx` (CSS Quirks §4.1: ячейка не
///     может быть *уже* указанного, но расширяться разрешено — table
///     layout не перегрузит ячейку по ширине)
///
/// `<td height="N">` / `<th height="N">` / `<table height="N">` → `height: Npx`
/// без quirks-вариации (HTML5 §14.3.9.1).
///
/// Процентные значения (`"50%"`) поддерживаются через `Length::Percent`.
pub(in crate::style) fn apply_table_cell_width_hint(doc: &Document, node: NodeId, style: &mut ComputedStyle) {
    let NodeData::Element { name, .. } = &doc.get(node).data else {
        return;
    };
    let tag = name.local.as_str();
    let is_cell = matches!(tag, "td" | "th");
    let is_table = tag == "table";
    if !is_cell && !is_table {
        return;
    }
    let node_ref = doc.get(node);
    if let Some(len) = node_ref.get_attr("width").and_then(parse_html_length_attr) {
        if is_cell && doc.mode() == DocumentMode::Quirks {
            // CSS Quirks §4.1: width attr на ячейке → min-width, не width.
            style.min_width = Some(len);
        } else {
            style.width = Some(len);
        }
    }
    if let Some(len) = node_ref.get_attr("height").and_then(parse_html_length_attr) {
        style.height = Some(len);
    }
}
/// Парсит HTML dimension-атрибут как `Length`.
///
/// `"200"` → `Length::Px(200.0)`, `"50%"` → `Length::Percent(50.0)`.
/// Мусор после цифр игнорируется (HTML5 §2.4.4.5).
pub(in crate::style) fn parse_html_length_attr(s: &str) -> Option<Length> {
    let s = s.trim();
    if let Some(pct) = s.strip_suffix('%') {
        let digits: String = pct.chars().take_while(|c| c.is_ascii_digit()).collect();
        digits.parse::<u32>().ok().map(|n| Length::Percent(n as f32))
    } else {
        parse_html_dimension(s).map(Length::Px)
    }
}
/// HTML5 §2.4.6 «rules for parsing a legacy color value».
///
/// Используется для presentational hint-атрибутов вроде `<body bgcolor>`,
/// `<td bgcolor>`, `<body text>`, `<font color>`. Алгоритм значительно
/// лояльнее CSS-парсера: принимает named colors, `#rgb` / `#rrggbb`,
/// hashless hex произвольной длины, и через padding/truncate process
/// выдаёт цвет из почти любой непустой строки, отличной от
/// «transparent».
///
/// Отказы (Spec: «error»):
/// - пустая строка / только whitespace;
/// - ASCII case-insensitive match «transparent».
///
/// Все остальные строки возвращают непустой цвет — это нужно для
/// совместимости с legacy-разметкой, где атрибуты часто содержат мусор.
///
/// Реализация работает в `Vec<char>` (Unicode code points), как требует
/// spec — не в байтах. Не-BMP code-point (> U+FFFF) заменяется на две
/// ASCII-«0» (spec step 6).
pub(in crate::style) fn parse_legacy_color_html_attr(input: &str) -> Option<Color> {
    // Step 1-2: empty → error.
    if input.is_empty() {
        return None;
    }
    // Step 3: strip leading/trailing ASCII whitespace.
    let trimmed = input.trim_matches(|c: char| matches!(c, '\t' | '\n' | '\x0C' | '\r' | ' '));
    if trimmed.is_empty() {
        return None;
    }
    // Step 4: case-insensitive «transparent» → error.
    if trimmed.eq_ignore_ascii_case("transparent") {
        return None;
    }
    // Step 5: named X11 / CSS3 color.
    let lc = trimmed.to_ascii_lowercase();
    // `named_color` принимает уже-lc имя и для «transparent» вернул бы
    // TRANSPARENT-константу — но мы уже отказали выше, так что попадание
    // невозможно.
    if let Some(c) = named_color(&lc) {
        return Some(c);
    }
    // Step 6: special-case 4-char `#xyz` short hex.
    let bytes = trimmed.as_bytes();
    if trimmed.len() == 4
        && bytes[0] == b'#'
        && bytes[1].is_ascii_hexdigit()
        && bytes[2].is_ascii_hexdigit()
        && bytes[3].is_ascii_hexdigit()
    {
        let r = hex_digit_value(bytes[1]) * 17;
        let g = hex_digit_value(bytes[2]) * 17;
        let b = hex_digit_value(bytes[3]) * 17;
        return Some(Color { r, g, b, a: 255 });
    }
    // Step 7: replace non-BMP code-points с двумя «0»; затем truncate до 128.
    let mut chars: Vec<char> = Vec::with_capacity(trimmed.len());
    for c in trimmed.chars() {
        if (c as u32) > 0xFFFF {
            chars.push('0');
            chars.push('0');
        } else {
            chars.push(c);
        }
    }
    if chars.len() > 128 {
        chars.truncate(128);
    }
    // Step 8: leading `#` удаляется.
    if !chars.is_empty() && chars[0] == '#' {
        chars.remove(0);
    }
    // Step 9: не-hex-digits заменяются на «0».
    for c in &mut chars {
        if !c.is_ascii_hexdigit() {
            *c = '0';
        }
    }
    // Step 10: padding нулями до длины > 0 и multiple of 3.
    while chars.is_empty() || !chars.len().is_multiple_of(3) {
        chars.push('0');
    }
    // Step 11: split на три равных компонента.
    let mut length = chars.len() / 3;
    let mut red: Vec<char> = chars[0..length].to_vec();
    let mut green: Vec<char> = chars[length..length * 2].to_vec();
    let mut blue: Vec<char> = chars[length * 2..length * 3].to_vec();
    // Step 12: если length > 8, оставляем только последние 8 (срезаем leading).
    if length > 8 {
        let skip = length - 8;
        red.drain(0..skip);
        green.drain(0..skip);
        blue.drain(0..skip);
        length = 8;
    }
    // Step 13: пока length > 2 и у всех трёх компонентов лидирующий «0» —
    // удаляем по «0» из каждого. Это «strip common leading zeros».
    while length > 2 && red[0] == '0' && green[0] == '0' && blue[0] == '0' {
        red.remove(0);
        green.remove(0);
        blue.remove(0);
        length -= 1;
    }
    // Step 14: если length всё ещё > 2, оставляем только первые 2.
    if length > 2 {
        red.truncate(2);
        green.truncate(2);
        blue.truncate(2);
    }
    // Step 15-19: parse hex.
    let r = u8::from_str_radix(&red.iter().collect::<String>(), 16).ok()?;
    let g = u8::from_str_radix(&green.iter().collect::<String>(), 16).ok()?;
    let b = u8::from_str_radix(&blue.iter().collect::<String>(), 16).ok()?;
    Some(Color { r, g, b, a: 255 })
}
/// Значение ASCII hex-digit как 0..=15. Caller гарантирует
/// `is_ascii_hexdigit()` — иначе возвращает 0.
pub(in crate::style) fn hex_digit_value(b: u8) -> u8 {
    match b {
        b'0'..=b'9' => b - b'0',
        b'a'..=b'f' => b - b'a' + 10,
        b'A'..=b'F' => b - b'A' + 10,
        _ => 0,
    }
}
/// HTML5 «rules for parsing dimension values»: unitless целое число
/// пикселей, опциональный trailing `%` (Phase 0 пропускаем процентный
/// случай — нужен containing-block-width). Отрицательные значения
/// невалидны.
pub(in crate::style) fn parse_html_dimension(s: &str) -> Option<f32> {
    let s = s.trim();
    // Процентные размеры пока не поддерживаем — требуют containing block.
    if s.ends_with('%') {
        return None;
    }
    // Берём префикс из цифр (HTML5 принимает мусор после), парсим как u32.
    let digits: String = s.chars().take_while(|c| c.is_ascii_digit()).collect();
    if digits.is_empty() {
        return None;
    }
    digits.parse::<u32>().ok().map(|n| n as f32)
}
