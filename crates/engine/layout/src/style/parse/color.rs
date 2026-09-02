//! Разбор CSS `<color>`: legacy/quirks-формы, `color()`, hex, `color-mix()`,
//! `color-contrast()`, относительные цвета с их микро-лексером [`ColorTok`] и
//! конверсии oklab/lab/oklch в sRGB.
//!
//! Перенесено батчем SPLIT-ST3 из `crates/engine/layout/src/style.rs`
//! (анкеры `fn parse_color` … `fn hue_to_rgb`) без правок тел: изменены только
//! пути модулей и видимость тех items, которые продолжают звать `style.rs` и
//! его тест-модули. Таблица `NAMED_COLORS` уехала отдельным табличным файлом —
//! [`crate::style::values::named_colors`].

use lumen_core::ColorSpace;

use crate::style::parse::timeline::tokenize_with_parens;
use crate::style::values::color::{encode_srgb_f32, predefined_to_srgb_linear};
use crate::style::values::named_colors::NAMED_COLORS;
use crate::style::{Color, ColorFloat, CssColor, SystemColor, split_top_level_commas};

/// Парсит CSS-значение `<color>` в непрозрачный [`Color`]: named color,
/// `#rgb`/`#rrggbb`/`#rrggbbaa` либо функциональную форму
/// (`rgb()`/`hsl()`/`oklch()`/`color()`/`color-mix()`/…).
///
/// Возвращает `None`, если значение не является цветом. Keyword-и уровня
/// каскада (`currentcolor`, системные цвета) здесь НЕ резолвятся — за них
/// отвечают [`parse_css_color_legacy`] и [`system_color`].
pub fn parse_color(s: &str) -> Option<Color> {
    let s = s.trim();
    if let Some(c) = named_color(&s.to_ascii_lowercase()) {
        return Some(c);
    }
    if let Some(c) = parse_hex_color(s) {
        return Some(c);
    }
    if let Some(c) = parse_function_color(s) {
        return Some(c);
    }
    // `color(<space> …)` — CSS Color L4 §10.1. Хранить его как `ColorFloat`
    // умеет только каскад ([`parse_css_color_legacy`] пробует эту ветку ПЕРВОЙ,
    // чтобы не терять wide-gamut точность); здесь она стоит последней и сразу
    // гамут-маппится в sRGB — для потребителей, у которых нет `CssColor`
    // (Canvas 2D, BUG-451).
    parse_css_color_fn(s).map(ColorFloat::to_srgb_color)
}

/// CSSOM specified-value serialization for a `<color>` assigned through
/// `CSSStyleDeclaration.setProperty`/`el.style[prop] = …` (CSS Color L4 §4.2,
/// CSSOM §6.7.3). Unlike [`parse_color`], which resolves everything down to
/// an opaque `Color`, this keeps keyword-syntax input (named colors,
/// `currentcolor`, `transparent`, system-color keywords, and the CSS-wide
/// keywords `inherit`/`initial`/`unset`/`revert`/`revert-layer`/`revert-rule`,
/// which are valid on any property and are not `<color>` syntax at all) serialized as
/// the keyword itself, lowercased — only hex/legacy-functional notation
/// (`rgb()`/`rgba()`/`hsl()`/`hsla()`/`hwb()`/…) canonicalizes to the
/// `rgb()`/`rgba()` functional form. Returns `None` when `s` is not a valid
/// `<color>` (nor a CSS-wide keyword) at all, so the caller can reject the
/// assignment instead of storing an unparsed string — the gap measured in
/// WPT `css/CSS2/syntax/colors-007.html` (BUG-465).
pub fn canonical_specified_color(s: &str) -> Option<String> {
    let trimmed = s.trim();
    if trimmed.is_empty() {
        return None;
    }
    let lower = trimmed.to_ascii_lowercase();
    if matches!(
        lower.as_str(),
        "inherit" | "initial" | "unset" | "revert" | "revert-layer" | "revert-rule"
    ) || lower == "currentcolor"
        || named_color(&lower).is_some()
        || SystemColor::parse(&lower).is_some()
    {
        return Some(lower);
    }
    if let Some(c) = parse_hex_color(trimmed) {
        return Some(crate::selector_query::color_to_css(c));
    }
    if let Some(c) = parse_function_color(trimmed) {
        return Some(crate::selector_query::color_to_css(c));
    }
    None
}

/// CSS Quirks Mode §3.4 «hashless hex color quirk».
///
/// В quirks-mode значение `<color>`, не парсящееся стандартным `parse_color`,
/// но состоящее ровно из 3, 6 или 8 ASCII hex-digits без ведущего `#`,
/// трактуется так, будто `#` присутствовал. То есть в `<body
/// style="color: ff0000">` цвет — красный, при условии что
/// `Document.mode() == Quirks`.
///
/// Длины 3/6/8 покрывают `#rgb` / `#rrggbb` / `#rrggbbaa`. Spec упоминает
/// также длины 7/9, но они появляются только из патологического
/// padding-парсинга «legacy color value» (HTML5 §2.4.6) и в реальных
/// браузерах не используются для CSS quirks.
///
/// В Standards / LimitedQuirks функция полностью эквивалентна `parse_color`.
pub(in crate::style) fn parse_color_legacy(s: &str, is_quirks: bool) -> Option<Color> {
    if let Some(c) = parse_color(s) {
        return Some(c);
    }
    if !is_quirks {
        return None;
    }
    let trimmed = s.trim();
    if trimmed.starts_with('#') {
        return None;
    }
    let len = trimmed.len();
    if !matches!(len, 3 | 6 | 8) {
        return None;
    }
    if !trimmed.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let with_hash = format!("#{trimmed}");
    parse_color(&with_hash)
}

/// Парсит `<color>` + `currentcolor` keyword в `CssColor`.
///
/// `currentcolor` — специальное CSS keyword, означающее «использовать
/// вычисленный `color` элемента при рендеринге». Возвращает
/// `Some(CssColor::CurrentColor)` для этого случая;
/// `Some(CssColor::Rgba(...))` для обычных цветов; `None` для невалидных.
pub(in crate::style) fn parse_css_color_legacy(s: &str, is_quirks: bool) -> Option<CssColor> {
    let s = s.trim();
    if s.eq_ignore_ascii_case("currentcolor") {
        return Some(CssColor::CurrentColor);
    }
    // CSS Color 4 §6.2 — system color keywords (Canvas, ButtonFace, …).
    // Stored as CssColor::System so the cascade post-pass can resolve them
    // against the element's used color scheme.
    if let Some(sc) = SystemColor::parse(s) {
        return Some(CssColor::System(sc));
    }
    // CSS Color L4 §10: color() function with predefined color spaces.
    if let Some(wide) = parse_css_color_fn(s) {
        return Some(CssColor::Wide(wide));
    }
    parse_color_legacy(s, is_quirks).map(CssColor::Rgba)
}

/// CSS Color L4 §10.1 — парсит `color(<space> c1 c2 c3 [/ alpha])`.
///
/// Displayable spaces — `srgb`, `display-p3`, `rec2020` — хранятся как
/// `ColorFloat` со своим `ColorSpace`, чтобы сохранить линейную точность для
/// GPU-paint. Остальные предопределённые пространства CSS Color L4 §10
/// (`srgb-linear`, `a98-rgb`, `prophoto-rgb`, `xyz`, `xyz-d65`, `xyz-d50`) не
/// представимы на sRGB-экране, поэтому гамут-маппятся в sRGB сразу при разборе
/// и хранятся как `ColorFloat { space: Srgb }` с gamma-encoded каналами.
///
/// CSS Color L5 §4 — `--<dashed-ident>` первым токеном ссылается на
/// `@color-profile`-профиль. Реальная ICC-трансформация и проверка, что имя
/// действительно объявлено, отложены (см. `ColorProfileRule` в css-parser);
/// каналы трактуются как уже sRGB, аналогично ветке `srgb`.
///
/// Каналы: unitless float или % (100% = 1.0). Слэш — разделитель alpha.
fn parse_css_color_fn(s: &str) -> Option<ColorFloat> {
    let lower = s.to_ascii_lowercase();
    let body = lower.strip_prefix("color(")?.strip_suffix(')')?;
    // Разбиваем по пробелам и слэшу, пропуская пустые токены.
    let tokens: Vec<&str> = body.split(|c: char| c.is_whitespace() || c == '/').filter(|t| !t.is_empty()).collect();
    if tokens.len() < 4 {
        return None;
    }
    let c1 = parse_color_fn_channel(tokens[1])?;
    let c2 = parse_color_fn_channel(tokens[2])?;
    let c3 = parse_color_fn_channel(tokens[3])?;
    let a = if tokens.len() >= 5 {
        parse_color_fn_channel(tokens[4])?.clamp(0.0, 1.0)
    } else {
        1.0
    };
    match tokens[0] {
        "srgb" => Some(ColorFloat { r: c1, g: c2, b: c3, a, space: ColorSpace::Srgb }),
        "display-p3" => Some(ColorFloat { r: c1, g: c2, b: c3, a, space: ColorSpace::DisplayP3 }),
        "rec2020" => Some(ColorFloat { r: c1, g: c2, b: c3, a, space: ColorSpace::Rec2020 }),
        // CSS Color L5 §4 — `color(--name c1 c2 c3)` referencing an
        // `@color-profile`-declared custom profile. Real ICC-based transform
        // (and existence validation against the declared profile name) is
        // deferred — channels are treated as already-encoded sRGB, same as
        // the `srgb` branch above.
        space if space.starts_with("--") => {
            Some(ColorFloat { r: c1, g: c2, b: c3, a, space: ColorSpace::Srgb })
        }
        other => {
            let (lr, lg, lb) = predefined_to_srgb_linear(other, c1, c2, c3)?;
            Some(ColorFloat {
                r: encode_srgb_f32(lr),
                g: encode_srgb_f32(lg),
                b: encode_srgb_f32(lb),
                a,
                space: ColorSpace::Srgb,
            })
        }
    }
}

/// Парсит channel для `color()`: unitless float или процент (100% = 1.0).
fn parse_color_fn_channel(s: &str) -> Option<f32> {
    let s = s.trim();
    if let Some(pct) = s.strip_suffix('%') {
        return pct.trim().parse::<f32>().ok().map(|p| p / 100.0);
    }
    if s == "none" {
        return Some(0.0);
    }
    s.parse::<f32>().ok()
}

/// CSS Color Module Level 3 §4.3 — X11 / SVG named colors. Принимает имя
/// уже в нижнем регистре. Возвращает None для неизвестного имени.
///
/// Реализовано бинарным поиском по сортированному списку: O(log n) на
/// lookup, no allocations, читается как табличный data-driven код.
/// `transparent` (CSS Color L3) — отдельная константа, потому что у него
/// alpha = 0. `currentcolor` не реализуется здесь — это keyword уровня
/// каскада, требующий доступа к computed `color`.
pub(in crate::style) fn named_color(name_lc: &str) -> Option<Color> {
    if name_lc == "transparent" {
        return Some(Color::TRANSPARENT);
    }
    NAMED_COLORS
        .binary_search_by_key(&name_lc, |&(n, _)| n)
        .ok()
        .map(|i| {
            let (_, (r, g, b)) = NAMED_COLORS[i];
            Color { r, g, b, a: 255 }
        })
}

/// CSS Color Module Level 4 §6.2 — резолв системных цветовых ключевых слов
/// (`Canvas`, `CanvasText`, `ButtonFace` и т.д.) в конкретный RGB.
///
/// `name_lc` — имя ключевого слова уже в нижнем регистре. `dark` — «used
/// color scheme» элемента (см. [`crate::style::ColorScheme::used_dark`]): системные цвета
/// контекстно-зависимы и резолвятся против темы элемента, а не глобально.
///
/// Значения подобраны под штатные light/dark палитры современных UA
/// (близко к Chrome/Edge). Возвращает `None` для не-системного имени —
/// caller трактует это как «не системный цвет» и идёт по обычному пути
/// `named_color` / hex / функция.
///
/// Этот резолвер — алгоритмическая часть; подключение к каскаду (вариант
/// `CssColor::System`, резолв на used-value time) — за P4 (`// CSS:
/// system-color` в `compute_style`).
#[must_use]
pub fn system_color(name_lc: &str, dark: bool) -> Option<Color> {
    let rgb = |r: u8, g: u8, b: u8| Color { r, g, b, a: 255 };
    Some(match (name_lc, dark) {
        // App content background / text.
        ("canvas", false) | ("window", false) => rgb(255, 255, 255),
        ("canvas", true) | ("window", true) => rgb(30, 30, 30),
        ("canvastext", false) | ("windowtext", false) | ("fieldtext", false) => rgb(0, 0, 0),
        ("canvastext", true) | ("windowtext", true) | ("fieldtext", true) => rgb(255, 255, 255),
        // Input/textarea/select backgrounds.
        ("field", false) => rgb(255, 255, 255),
        ("field", true) => rgb(30, 30, 30),
        // Push-button surfaces. Light values match Chromium/Edge non-forced-colors
        // light theme (verified against TEST-92 Edge capture).
        ("buttonface", false) => rgb(240, 240, 240),
        ("buttonface", true) => rgb(58, 58, 60),
        ("buttontext", false) => rgb(0, 0, 0),
        ("buttontext", true) => rgb(255, 255, 255),
        // ButtonBorder in Edge's light theme is pure black, not mid-gray.
        ("buttonborder", false) | ("threedface", false) => rgb(0, 0, 0),
        ("buttonborder", true) | ("threedface", true) => rgb(97, 97, 97),
        // Hyperlinks. Edge resolves LinkText/VisitedText/ActiveText to the same
        // #0066cc in light theme (visited/active are anti-fingerprinting clamped).
        ("linktext", false) => rgb(0, 102, 204),
        ("linktext", true) => rgb(158, 158, 255),
        ("visitedtext", false) => rgb(0, 102, 204),
        ("visitedtext", true) => rgb(209, 134, 255),
        ("activetext", false) => rgb(0, 102, 204),
        ("activetext", true) => rgb(255, 158, 158),
        // Selection highlight. Edge light Highlight = #0078d7 (classic accent),
        // with white HighlightText on top.
        ("highlight", false) => rgb(0, 120, 215),
        ("highlight", true) => rgb(38, 79, 120),
        ("highlighttext", false) => rgb(255, 255, 255),
        ("highlighttext", true) => rgb(255, 255, 255),
        ("selecteditem", false) => rgb(0, 120, 215),
        ("selecteditem", true) => rgb(38, 79, 120),
        ("selecteditemtext", false) => rgb(255, 255, 255),
        ("selecteditemtext", true) => rgb(255, 255, 255),
        // Disabled / placeholder text. Edge light GrayText = #6d6d6d.
        ("graytext", false) | ("greytext", false) => rgb(109, 109, 109),
        ("graytext", true) | ("greytext", true) => rgb(124, 124, 124),
        // <mark> highlight (CSS Color 4 §6.2 `Mark`/`MarkText`).
        ("mark", false) => rgb(255, 255, 0),
        ("mark", true) => rgb(255, 255, 0),
        ("marktext", false) | ("marktext", true) => rgb(0, 0, 0),
        // Accent colour for form controls (CSS Color 4 — `AccentColor`).
        // Edge light AccentColor = #0075ff.
        ("accentcolor", false) => rgb(0, 117, 255),
        ("accentcolor", true) => rgb(76, 156, 255),
        ("accentcolortext", false) | ("accentcolortext", true) => rgb(255, 255, 255),
        // Deprecated CSS2 3D effects (CSS Color 4 §6.3): all map to ButtonBorder.
        // Edge renders ThreeDHighlight/ThreeDShadow as #000000 in light theme.
        ("threedhighlight", false) => rgb(0, 0, 0),
        ("threedhighlight", true) => rgb(97, 97, 97),
        ("threedshadow", false) => rgb(0, 0, 0),
        ("threedshadow", true) => rgb(97, 97, 97),
        ("threedlightshadow", false) => rgb(0, 0, 0),
        ("threedlightshadow", true) => rgb(97, 97, 97),
        ("threeddarkshadow", false) => rgb(0, 0, 0),
        ("threeddarkshadow", true) => rgb(97, 97, 97),
        // Deprecated Scrollbar (CSS Color 4 §6.3) maps to Canvas.
        ("scrollbar", false) => rgb(255, 255, 255),
        ("scrollbar", true) => rgb(30, 30, 30),
        ("scrollbartrack", false) => rgb(255, 255, 255),
        ("scrollbartrack", true) => rgb(30, 30, 30),
        ("scrollbarthumb", false) => rgb(192, 192, 192),
        ("scrollbarthumb", true) => rgb(97, 97, 97),
        _ => return None,
    })
}


fn parse_hex_color(s: &str) -> Option<Color> {
    let hex = s.strip_prefix('#')?;
    // Все ветки ниже режут `hex` БАЙТОВЫМИ индексами, а `len()` — тоже в
    // байтах, поэтому один не-ASCII символ и проходит проверку длины, и рвёт
    // границу UTF-8: `#±a` — три байта, ветка `3`, `&hex[0..1]` внутри '±'
    // → паника (BUG-451, найдено пробой; достижимо из любого стиля страницы,
    // не только из Canvas 2D).
    if !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    match hex.len() {
        3 => {
            let r = u8::from_str_radix(&hex[0..1], 16).ok()?;
            let g = u8::from_str_radix(&hex[1..2], 16).ok()?;
            let b = u8::from_str_radix(&hex[2..3], 16).ok()?;
            // #RGB → #RRGGBB: каждый ниббл дублируется.
            Some(Color { r: r * 17, g: g * 17, b: b * 17, a: 255 })
        }
        4 => {
            // #RGBA — CSS4: каждый ниббл дублируется.
            let r = u8::from_str_radix(&hex[0..1], 16).ok()?;
            let g = u8::from_str_radix(&hex[1..2], 16).ok()?;
            let b = u8::from_str_radix(&hex[2..3], 16).ok()?;
            let a = u8::from_str_radix(&hex[3..4], 16).ok()?;
            Some(Color { r: r * 17, g: g * 17, b: b * 17, a: a * 17 })
        }
        6 => {
            let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
            let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
            let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
            Some(Color { r, g, b, a: 255 })
        }
        8 => {
            let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
            let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
            let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
            let a = u8::from_str_radix(&hex[6..8], 16).ok()?;
            Some(Color { r, g, b, a })
        }
        _ => None,
    }
}

/// Парсит `rgb(…)`, `rgba(…)`, `hsl(…)`, `hsla(…)`. Поддерживает запятые
/// и whitespace как разделители, как `rgb`/`rgba` синонимы, так и `hsl`/`hsla`.
/// Компоненты:
///   - rgb: целое 0–255 или процент 0–100% (для каждого канала);
///   - hsl: hue в градусах (число или `<n>deg`), saturation и lightness в %;
///   - alpha (4-й компонент): float 0..1 или процент 0–100%. По умолчанию 1.
fn parse_function_color(s: &str) -> Option<Color> {
    let lower = s.to_ascii_lowercase();
    // CSS Color L5 §10.2 color-mix().
    if lower.starts_with("color-mix(") && s.ends_with(')') {
        return parse_color_mix(&s["color-mix(".len()..s.len() - 1]);
    }
    // CSS Color L5 §11 color-contrast().
    if lower.starts_with("color-contrast(") && s.ends_with(')') {
        return parse_color_contrast(&s["color-contrast(".len()..s.len() - 1]);
    }
    // CSS Color L4 §7 hwb(). Отдельной веткой, а не вариантом `ColorFn`:
    // относительная форма `hwb(from …)` требует канала в
    // `relative_origin_channels`, которого у `MixColorSpace::Hwb` нет (там
    // ветка `_ => srgb`), и вариант молча давал бы неверный результат.
    if let Some(b) = lower.strip_prefix("hwb(").and_then(|t| t.strip_suffix(')')) {
        return parse_hwb_body(b);
    }
    let (kind, body) = if let Some(b) = lower.strip_prefix("rgba(").and_then(|t| t.strip_suffix(')')) {
        (ColorFn::Rgb, b)
    } else if let Some(b) = lower.strip_prefix("rgb(").and_then(|t| t.strip_suffix(')')) {
        (ColorFn::Rgb, b)
    } else if let Some(b) = lower.strip_prefix("hsla(").and_then(|t| t.strip_suffix(')')) {
        (ColorFn::Hsl, b)
    } else if let Some(b) = lower.strip_prefix("hsl(").and_then(|t| t.strip_suffix(')')) {
        (ColorFn::Hsl, b)
    } else if let Some(b) = lower.strip_prefix("oklch(").and_then(|t| t.strip_suffix(')')) {
        (ColorFn::Oklch, b)
    } else if let Some(b) = lower.strip_prefix("oklab(").and_then(|t| t.strip_suffix(')')) {
        (ColorFn::Oklab, b)
    } else if let Some(b) = lower.strip_prefix("lab(").and_then(|t| t.strip_suffix(')')) {
        (ColorFn::Lab, b)
    } else {
        let b = lower.strip_prefix("lch(").and_then(|t| t.strip_suffix(')'))?;
        (ColorFn::Lch, b)
    };
    // CSS Color L5 §4 — relative color: `<fn>(from <origin> c1 c2 c3 [/ a])`.
    if let Some(rest) = body.trim_start().strip_prefix("from ") {
        return parse_relative_color(kind, rest.trim());
    }
    let parts = split_color_args(body);
    if !(parts.len() == 3 || parts.len() == 4) {
        return None;
    }
    let alpha = if parts.len() == 4 {
        parse_alpha_component(&parts[3])?
    } else {
        255
    };
    match kind {
        ColorFn::Rgb => {
            let r = parse_rgb_component(&parts[0])?;
            let g = parse_rgb_component(&parts[1])?;
            let b = parse_rgb_component(&parts[2])?;
            Some(Color { r, g, b, a: alpha })
        }
        ColorFn::Hsl => {
            let h = parse_hue_component(&parts[0])?;
            let s = parse_percent_component(&parts[1])?;
            let l = parse_percent_component(&parts[2])?;
            let (r, g, b) = hsl_to_rgb(h, s, l);
            Some(Color { r, g, b, a: alpha })
        }
        ColorFn::Oklch => {
            // L: 0..1 как число или 0..100% (в spec L=0%..100% соответствует 0..1).
            let l = parse_oklch_lightness(&parts[0])?;
            // C: число или процент (100% = 0.4 по spec L4 §10.3 reference range).
            let c = parse_oklch_chroma(&parts[1])?;
            let h = parse_hue_component(&parts[2])?;
            let (r, g, b) = oklch_to_srgb(l, c, h);
            Some(Color { r, g, b, a: alpha })
        }
        ColorFn::Oklab => {
            // OKLab: L=0..1, a/b — unitless (~±0.4). 100% для a/b = ±0.4.
            let l = parse_oklch_lightness(&parts[0])?;
            let a = parse_oklab_ab(&parts[1])?;
            let b = parse_oklab_ab(&parts[2])?;
            let (r, g, b) = oklab_to_srgb(l, a, b);
            Some(Color { r, g, b, a: alpha })
        }
        ColorFn::Lab => {
            // CIE Lab (D50): L=0..100, a/b — unitless (~±125). 100% = ±125.
            let l = parse_lab_lightness(&parts[0])?;
            let a = parse_lab_ab(&parts[1])?;
            let b = parse_lab_ab(&parts[2])?;
            let (r, g, b) = lab_to_srgb(l, a, b);
            Some(Color { r, g, b, a: alpha })
        }
        ColorFn::Lch => {
            // LCH: L=0..100, C≥0 (100% = 150), H в градусах.
            let l = parse_lab_lightness(&parts[0])?;
            let c = parse_lch_chroma(&parts[1])?;
            let h = parse_hue_component(&parts[2])?;
            let h_rad = h.to_radians();
            let a = c * h_rad.cos();
            let b_v = c * h_rad.sin();
            let (r, g, b) = lab_to_srgb(l, a, b_v);
            Some(Color { r, g, b, a: alpha })
        }
    }
}

/// CSS Color L4 §7 — тело `hwb(<hue> <whiteness>% <blackness>% [/ <alpha>])`
/// (без имени функции и скобок, уже в нижнем регистре).
///
/// Формула спеки: при `w + b >= 1` цвет — серый `w / (w + b)`, иначе чистый тон
/// (`hsl(h 100% 50%)`) сжимается в интервал `[w, 1 - b]`. Относительная форма
/// (`hwb(from …)`) не поддержана и отвергается — см. вызывающую ветку.
fn parse_hwb_body(body: &str) -> Option<Color> {
    if body.trim_start().starts_with("from ") {
        return None;
    }
    let parts = split_color_args(body);
    if !(parts.len() == 3 || parts.len() == 4) {
        return None;
    }
    let alpha = if parts.len() == 4 {
        parse_alpha_component(&parts[3])?
    } else {
        255
    };
    let h = parse_hue_component(&parts[0])?;
    let w = parse_percent_component(&parts[1])?;
    let bk = parse_percent_component(&parts[2])?;
    if w + bk >= 1.0 {
        let gray = clamp_byte(w / (w + bk) * 255.0);
        return Some(Color { r: gray, g: gray, b: gray, a: alpha });
    }
    let (pr, pg, pb) = hsl_to_rgb(h, 1.0, 0.5);
    let span = 1.0 - w - bk;
    let mix = |c: u8| clamp_byte((f32::from(c) / 255.0 * span + w) * 255.0);
    Some(Color { r: mix(pr), g: mix(pg), b: mix(pb), a: alpha })
}

#[derive(Clone, Copy)]
enum ColorFn {
    Rgb,
    Hsl,
    Oklch,
    Oklab,
    Lab,
    Lch,
    // Прочие CSS4 расширения (color()) — позже.
}

impl ColorFn {
    /// Interpolation space whose channels back this color function — used to
    /// resolve relative-color origin channels (CSS Color L5 §4.1).
    fn mix_space(self) -> crate::color_mix::MixColorSpace {
        use crate::color_mix::MixColorSpace as M;
        match self {
            ColorFn::Rgb => M::Srgb,
            ColorFn::Hsl => M::Hsl,
            ColorFn::Oklch => M::Oklch,
            ColorFn::Oklab => M::Oklab,
            ColorFn::Lab => M::Lab,
            ColorFn::Lch => M::Lch,
        }
    }

    /// The three relative-color channel keyword names, in component order
    /// (CSS Color L5 §4.1). `alpha` is always available as a fourth keyword.
    fn channel_keywords(self) -> [&'static str; 3] {
        match self {
            ColorFn::Rgb => ["r", "g", "b"],
            ColorFn::Hsl => ["h", "s", "l"],
            ColorFn::Oklch | ColorFn::Lch => ["l", "c", "h"],
            ColorFn::Oklab | ColorFn::Lab => ["l", "a", "b"],
        }
    }

    /// Percent reference basis for each of the three components: a `<percentage>`
    /// in a component resolves to `pct/100 * basis` in the channel's canonical
    /// unit (CSS Color L4 §10 reference ranges). `0.0` marks a hue slot where
    /// percentages are invalid.
    fn channel_pct_basis(self) -> [f32; 3] {
        match self {
            ColorFn::Rgb => [255.0, 255.0, 255.0],
            ColorFn::Hsl => [0.0, 100.0, 100.0],
            ColorFn::Lab => [100.0, 125.0, 125.0],
            ColorFn::Lch => [100.0, 150.0, 0.0],
            ColorFn::Oklab => [1.0, 0.4, 0.4],
            ColorFn::Oklch => [1.0, 0.4, 0.0],
        }
    }

    /// Rebuild a non-relative color function string from resolved canonical
    /// channel values + alpha (0–1), to be re-parsed by [`parse_function_color`].
    fn format_resolved(self, c: [f32; 3], alpha: f32) -> String {
        match self {
            ColorFn::Rgb => format!("rgb({} {} {} / {})", c[0], c[1], c[2], alpha),
            // s / l are percentages in hsl().
            ColorFn::Hsl => format!("hsl({} {}% {}% / {})", c[0], c[1], c[2], alpha),
            ColorFn::Lab => format!("lab({} {} {} / {})", c[0], c[1], c[2], alpha),
            ColorFn::Lch => format!("lch({} {} {} / {})", c[0], c[1], c[2], alpha),
            ColorFn::Oklab => format!("oklab({} {} {} / {})", c[0], c[1], c[2], alpha),
            ColorFn::Oklch => format!("oklch({} {} {} / {})", c[0], c[1], c[2], alpha),
        }
    }
}

/// CSS Color L5 §4 — parse the relative-color body `<origin> c1 c2 c3 [/ a]`
/// (without the surrounding `<fn>(from ` and `)`), for the color function
/// `kind`. The origin color is parsed recursively and converted into `kind`'s
/// channel space; each component keyword (`r`/`g`/`b`, `h`/`s`/`l`, …, plus
/// `alpha`) resolves to the origin's channel value, optionally combined with
/// numbers/percentages inside `calc()`. Returns `None` on any parse error.
fn parse_relative_color(kind: ColorFn, body: &str) -> Option<Color> {
    let toks = tokenize_with_parens(body);
    if toks.len() < 4 {
        return None;
    }
    let origin = parse_color(&toks[0])?;
    let comps = &toks[1..];
    // Modern slash-separated alpha: `c1 c2 c3 / a`.
    let (chan_toks, alpha_tok): (&[String], Option<&str>) =
        if let Some(i) = comps.iter().position(|t| t == "/") {
            (&comps[..i], comps.get(i + 1).map(String::as_str))
        } else {
            (comps, None)
        };
    if chan_toks.len() != 3 {
        return None;
    }

    let srgb = [
        f32::from(origin.r) / 255.0,
        f32::from(origin.g) / 255.0,
        f32::from(origin.b) / 255.0,
        f32::from(origin.a) / 255.0,
    ];
    let chans = crate::color_mix::relative_origin_channels(kind.mix_space(), srgb);
    let names = kind.channel_keywords();
    let vars: [(&str, f32); 4] = [
        (names[0], chans[0]),
        (names[1], chans[1]),
        (names[2], chans[2]),
        ("alpha", chans[3]),
    ];
    let basis = kind.channel_pct_basis();
    let mut resolved = [0.0f32; 3];
    for (i, slot) in resolved.iter_mut().enumerate() {
        *slot = eval_color_component(&chan_toks[i], &vars, basis[i])?;
    }
    // alpha uses a 0–1 reference basis (100% = 1.0).
    let alpha = match alpha_tok {
        Some(a) => eval_color_component(a, &vars, 1.0)?,
        None => chans[3],
    };
    parse_function_color(&kind.format_resolved(resolved, alpha))
}

/// Resolve one relative-color component to its canonical channel value.
///
/// Accepts a bare channel keyword (`r`, `h`, `alpha`, …), `none` (→ 0), a
/// literal number / `<percentage>` / `<angle>`, or a `calc()` expression mixing
/// keywords, numbers and percentages with `+ - * /` and parentheses. `pct_basis`
/// is the channel's percent reference (`50%` → `0.5 * pct_basis`).
fn eval_color_component(raw: &str, vars: &[(&str, f32)], pct_basis: f32) -> Option<f32> {
    // `calc(` wrappers (including nested) become plain parens; bare tokens pass through.
    let normalized = raw.trim().replace("calc(", "(");
    let tokens = tokenize_color_expr(&normalized, pct_basis)?;
    let mut pos = 0usize;
    let v = eval_color_add(&tokens, &mut pos, vars)?;
    if pos != tokens.len() {
        return None;
    }
    Some(v)
}

/// Token in a relative-color component expression.
enum ColorTok {
    /// A numeric literal with its unit already resolved to a canonical value.
    Val(f32),
    /// A bare identifier (channel keyword, `alpha`, or `none`).
    Ident(String),
    /// One of `+ - * /`.
    Op(char),
    Open,
    Close,
}

/// Tokenize a relative-color component expression, resolving number units
/// (`%` → `pct_basis`, `deg`/`turn`/`grad`/`rad` → degrees) at scan time.
fn tokenize_color_expr(s: &str, pct_basis: f32) -> Option<Vec<ColorTok>> {
    let bytes = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < bytes.len() {
        let c = bytes[i] as char;
        if c.is_whitespace() {
            i += 1;
            continue;
        }
        match c {
            '(' => {
                out.push(ColorTok::Open);
                i += 1;
            }
            ')' => {
                out.push(ColorTok::Close);
                i += 1;
            }
            '+' | '-' | '*' | '/' => {
                out.push(ColorTok::Op(c));
                i += 1;
            }
            _ if c.is_ascii_digit() || c == '.' => {
                let start = i;
                while i < bytes.len() && ((bytes[i] as char).is_ascii_digit() || bytes[i] == b'.') {
                    i += 1;
                }
                let num: f32 = s[start..i].parse().ok()?;
                let unit_start = i;
                while i < bytes.len() && ((bytes[i] as char).is_ascii_alphabetic() || bytes[i] == b'%') {
                    i += 1;
                }
                let val = apply_color_unit(num, &s[unit_start..i], pct_basis)?;
                out.push(ColorTok::Val(val));
            }
            _ if c.is_ascii_alphabetic() => {
                let start = i;
                while i < bytes.len() && (bytes[i] as char).is_ascii_alphabetic() {
                    i += 1;
                }
                out.push(ColorTok::Ident(s[start..i].to_string()));
            }
            _ => return None,
        }
    }
    Some(out)
}

/// Convert a numeric literal + unit suffix to a canonical channel value.
fn apply_color_unit(num: f32, unit: &str, pct_basis: f32) -> Option<f32> {
    match unit {
        "" => Some(num),
        "%" => Some(num / 100.0 * pct_basis),
        "deg" => Some(num),
        "turn" => Some(num * 360.0),
        "grad" => Some(num * 0.9),
        "rad" => Some(num.to_degrees()),
        _ => None,
    }
}

fn eval_color_add(tokens: &[ColorTok], pos: &mut usize, vars: &[(&str, f32)]) -> Option<f32> {
    let mut v = eval_color_mul(tokens, pos, vars)?;
    while let Some(ColorTok::Op(op @ ('+' | '-'))) = tokens.get(*pos) {
        let op = *op;
        *pos += 1;
        let rhs = eval_color_mul(tokens, pos, vars)?;
        v = if op == '+' { v + rhs } else { v - rhs };
    }
    Some(v)
}

fn eval_color_mul(tokens: &[ColorTok], pos: &mut usize, vars: &[(&str, f32)]) -> Option<f32> {
    let mut v = eval_color_unary(tokens, pos, vars)?;
    while let Some(ColorTok::Op(op @ ('*' | '/'))) = tokens.get(*pos) {
        let op = *op;
        *pos += 1;
        let rhs = eval_color_unary(tokens, pos, vars)?;
        if op == '*' {
            v *= rhs;
        } else {
            if rhs == 0.0 {
                return None;
            }
            v /= rhs;
        }
    }
    Some(v)
}

fn eval_color_unary(tokens: &[ColorTok], pos: &mut usize, vars: &[(&str, f32)]) -> Option<f32> {
    match tokens.get(*pos) {
        Some(ColorTok::Op('-')) => {
            *pos += 1;
            eval_color_unary(tokens, pos, vars).map(|v| -v)
        }
        Some(ColorTok::Op('+')) => {
            *pos += 1;
            eval_color_unary(tokens, pos, vars)
        }
        _ => eval_color_primary(tokens, pos, vars),
    }
}

fn eval_color_primary(tokens: &[ColorTok], pos: &mut usize, vars: &[(&str, f32)]) -> Option<f32> {
    match tokens.get(*pos)? {
        ColorTok::Val(v) => {
            *pos += 1;
            Some(*v)
        }
        ColorTok::Ident(name) => {
            *pos += 1;
            if name.eq_ignore_ascii_case("none") {
                return Some(0.0);
            }
            vars.iter()
                .find(|(k, _)| k.eq_ignore_ascii_case(name))
                .map(|(_, val)| *val)
        }
        ColorTok::Open => {
            *pos += 1;
            let v = eval_color_add(tokens, pos, vars)?;
            match tokens.get(*pos) {
                Some(ColorTok::Close) => {
                    *pos += 1;
                    Some(v)
                }
                _ => None,
            }
        }
        _ => None,
    }
}

/// Парсит lightness для oklch: число 0..1 или процент 0..100% → 0..1.
fn parse_oklch_lightness(s: &str) -> Option<f32> {
    let s = s.trim();
    if let Some(pct) = s.strip_suffix('%') {
        return pct.trim().parse::<f32>().ok().map(|p| (p / 100.0).clamp(0.0, 1.0));
    }
    s.parse::<f32>().ok().map(|v| v.clamp(0.0, 1.0))
}

/// Парсит chroma для oklch: число (0..~0.4 типично) или процент (100% = 0.4).
fn parse_oklch_chroma(s: &str) -> Option<f32> {
    let s = s.trim();
    if let Some(pct) = s.strip_suffix('%') {
        // CSS Color L4 §10.3: 100% = 0.4.
        return pct.trim().parse::<f32>().ok().map(|p| (p / 100.0 * 0.4).max(0.0));
    }
    s.parse::<f32>().ok().map(|v| v.max(0.0))
}

/// Парсит a/b для oklab: число (~±0.4) или процент (100% = 0.4).
fn parse_oklab_ab(s: &str) -> Option<f32> {
    let s = s.trim();
    if let Some(pct) = s.strip_suffix('%') {
        // CSS Color L4 §10.4: 100% = 0.4 для a/b.
        return pct.trim().parse::<f32>().ok().map(|p| p / 100.0 * 0.4);
    }
    s.parse::<f32>().ok()
}

/// Парсит lightness для CIE Lab/LCH: число 0..100 или процент 0..100%.
fn parse_lab_lightness(s: &str) -> Option<f32> {
    let s = s.trim();
    if let Some(pct) = s.strip_suffix('%') {
        return pct.trim().parse::<f32>().ok().map(|p| p.clamp(0.0, 100.0));
    }
    s.parse::<f32>().ok().map(|v| v.clamp(0.0, 100.0))
}

/// Парсит a/b для CIE Lab: число (~±125) или процент (100% = 125).
fn parse_lab_ab(s: &str) -> Option<f32> {
    let s = s.trim();
    if let Some(pct) = s.strip_suffix('%') {
        // CSS Color L4 §10.5: 100% = 125.
        return pct.trim().parse::<f32>().ok().map(|p| p / 100.0 * 125.0);
    }
    s.parse::<f32>().ok()
}

/// Парсит chroma для LCH: число (≥0, ~0..230) или процент (100% = 150).
fn parse_lch_chroma(s: &str) -> Option<f32> {
    let s = s.trim();
    if let Some(pct) = s.strip_suffix('%') {
        // CSS Color L4 §10.5: 100% = 150 для LCH.
        return pct.trim().parse::<f32>().ok().map(|p| (p / 100.0 * 150.0).max(0.0));
    }
    s.parse::<f32>().ok().map(|v| v.max(0.0))
}

/// CSS Color L4 §10.4: OKLab напрямую → linear sRGB → gamma sRGB.
/// `l` ∈ [0,1], `a`/`b` — unitless. Алгоритм — second half of oklch_to_srgb.
fn oklab_to_srgb(l: f32, a: f32, b: f32) -> (u8, u8, u8) {
    let l_ = l + 0.396_337_77 * a + 0.215_803_76 * b;
    let m_ = l - 0.105_561_35 * a - 0.063_854_17 * b;
    let s_ = l - 0.089_484_18 * a - 1.291_485_5 * b;
    let l3 = l_ * l_ * l_;
    let m3 = m_ * m_ * m_;
    let s3 = s_ * s_ * s_;
    let lr = 4.076_741_7 * l3 - 3.307_711_6 * m3 + 0.230_969_94 * s3;
    let lg = -1.268_438 * l3 + 2.609_757_4 * m3 - 0.341_319_38 * s3;
    let lb = -0.004_196_086 * l3 - 0.703_418_6 * m3 + 1.707_614_7 * s3;
    (encode_srgb(lr), encode_srgb(lg), encode_srgb(lb))
}

/// CSS Color L4 §10.5: CIE Lab (D50) → XYZ → D65 (Bradford) → linear sRGB.
/// `l` ∈ [0,100], `a`/`b` — unitless (CIE units, не процентные).
fn lab_to_srgb(l: f32, a: f32, b: f32) -> (u8, u8, u8) {
    // Lab → XYZ (D50). Алгоритм CIE 15.3 §8.4.2.
    let fy = (l + 16.0) / 116.0;
    let fx = a / 500.0 + fy;
    let fz = fy - b / 200.0;
    let epsilon = 216.0 / 24389.0; // ≈ 0.008856
    let kappa = 24389.0 / 27.0; // ≈ 903.3
    let cube_or_linear = |f: f32, scaled: f32| -> f32 {
        let cubed = f * f * f;
        if cubed > epsilon {
            cubed
        } else {
            scaled / kappa
        }
    };
    let yr = if l > kappa * epsilon {
        let v = (l + 16.0) / 116.0;
        v * v * v
    } else {
        l / kappa
    };
    let xr = cube_or_linear(fx, 116.0 * fx - 16.0);
    let zr = cube_or_linear(fz, 116.0 * fz - 16.0);
    // D50 reference white (CIE 15.3 illuminant D50).
    let xn = 0.964_22;
    let yn = 1.0;
    let zn = 0.825_21;
    let x_d50 = xr * xn;
    let y_d50 = yr * yn;
    let z_d50 = zr * zn;
    // Bradford D50→D65 adaptation (CSS Color L4 §11).
    let x_d65 = 0.955_576_6 * x_d50 - 0.023_039_3 * y_d50 + 0.063_163_6 * z_d50;
    let y_d65 = -0.028_289_5 * x_d50 + 1.009_941_6 * y_d50 + 0.021_007_7 * z_d50;
    let z_d65 = 0.012_298_2 * x_d50 - 0.020_483_0 * y_d50 + 1.329_909_8 * z_d50;
    // D65 XYZ → linear sRGB (sRGB primary matrix, CIE 1931).
    let lr = 3.240_625_5 * x_d65 - 1.537_208 * y_d65 - 0.498_628_6 * z_d65;
    let lg = -0.968_930_7 * x_d65 + 1.875_756_1 * y_d65 + 0.041_517_5 * z_d65;
    let lb = 0.055_710_1 * x_d65 - 0.204_021_1 * y_d65 + 1.056_995_9 * z_d65;
    (encode_srgb(lr), encode_srgb(lg), encode_srgb(lb))
}

/// Linear sRGB → gamma sRGB (IEC 61966-2-1).
pub(in crate::style) fn encode_srgb(c: f32) -> u8 {
    let c = c.clamp(0.0, 1.0);
    let v = if c <= 0.003_130_8 {
        12.92 * c
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    };
    (v * 255.0 + 0.5).clamp(0.0, 255.0) as u8
}

/// CSS Color L4 §10.3: OKLCH → OKLab → linear sRGB → sRGB (gamma-encoded).
/// `l` ∈ [0,1], `c` ≥ 0, `h_deg` в градусах.
fn oklch_to_srgb(l: f32, c: f32, h_deg: f32) -> (u8, u8, u8) {
    // OKLCH → OKLab.
    let h_rad = h_deg.to_radians();
    let a = c * h_rad.cos();
    let b = c * h_rad.sin();

    // OKLab → linear LMS → linear sRGB. Константы из CSS Color L4 §10.3,
    // округлены до f32-precision.
    let l_ = l + 0.396_337_77 * a + 0.215_803_76 * b;
    let m_ = l - 0.105_561_35 * a - 0.063_854_17 * b;
    let s_ = l - 0.089_484_18 * a - 1.291_485_5 * b;
    let l3 = l_ * l_ * l_;
    let m3 = m_ * m_ * m_;
    let s3 = s_ * s_ * s_;
    let lr = 4.076_741_7 * l3 - 3.307_711_6 * m3 + 0.230_969_94 * s3;
    let lg = -1.268_438 * l3 + 2.609_757_4 * m3 - 0.341_319_38 * s3;
    let lb = -0.004_196_086 * l3 - 0.703_418_6 * m3 + 1.707_614_7 * s3;

    // Linear sRGB → gamma sRGB (per IEC 61966-2-1).
    fn encode(c: f32) -> u8 {
        let c = c.clamp(0.0, 1.0);
        let v = if c <= 0.003_130_8 {
            12.92 * c
        } else {
            1.055 * c.powf(1.0 / 2.4) - 0.055
        };
        clamp_byte(v * 255.0)
    }
    (encode(lr), encode(lg), encode(lb))
}

/// CSS Color L5 §10.2 — parse `color-mix(in <space>, <c1> [pct]?, <c2> [pct]?)`
/// from the inner body (without outer `color-mix(` and `)`).
/// Returns `None` on any parse error; invalid inputs are silently ignored per spec.
fn parse_color_mix(body: &str) -> Option<Color> {
    let parts = split_top_level_commas(body);
    if parts.len() != 3 {
        return None;
    }
    // Part 0: "in <space>" — case-insensitive per CSS Values §3.
    let part0 = parts[0].trim().to_ascii_lowercase();
    let space_str = part0.strip_prefix("in ")?.trim();
    let space = crate::color_mix::MixColorSpace::from_css(space_str)?;

    let (c1, w1_raw) = parse_color_with_pct(parts[1].trim())?;
    let (c2, w2_raw) = parse_color_with_pct(parts[2].trim())?;

    // Normalize weights: CSS Color L5 §10.2 §3 weight normalization.
    let (w1, w2) = match (w1_raw, w2_raw) {
        (None, None) => (0.5, 0.5),
        (Some(w), None) => (w, 1.0 - w),
        (None, Some(w)) => (1.0 - w, w),
        (Some(w1), Some(w2)) => (w1, w2),
    };

    let to_f = |c: Color| -> [f32; 4] {
        [
            c.r as f32 / 255.0,
            c.g as f32 / 255.0,
            c.b as f32 / 255.0,
            c.a as f32 / 255.0,
        ]
    };
    let out = crate::color_mix::mix_colors(space, to_f(c1), w1, to_f(c2), w2);
    Some(Color {
        r: (out[0] * 255.0).round().clamp(0.0, 255.0) as u8,
        g: (out[1] * 255.0).round().clamp(0.0, 255.0) as u8,
        b: (out[2] * 255.0).round().clamp(0.0, 255.0) as u8,
        a: (out[3] * 255.0).round().clamp(0.0, 255.0) as u8,
    })
}

/// CSS Color L5 §11 — `color-contrast( <color> vs <color>#{2,} [ to <target> ]? )`.
///
/// Picks the candidate color that contrasts best against the base color using
/// the WCAG 2.1 contrast-ratio formula. Without a `to <target>` clause the
/// candidate with the highest contrast is returned. With a target (either a
/// keyword — `AA`/`AA-large`/`AAA`/`AAA-large` — or a bare `<number>` ratio),
/// the first candidate in list order that meets or exceeds the target is
/// returned; if none reach it, the highest-contrast candidate is used.
///
/// Returns `None` when the syntax is malformed (missing `vs`, unparsable
/// color, fewer than two candidates, or an invalid target).
fn parse_color_contrast(body: &str) -> Option<Color> {
    // Split base from the candidate list on the top-level `vs` keyword.
    let (vs_start, vs_end) = find_keyword_at_depth0(body, "vs")?;
    let base = parse_color(body[..vs_start].trim())?;
    let rest = &body[vs_end..];

    // Optional trailing `to <target>` clause (top-level `to` keyword).
    let (list_str, target) = match find_keyword_at_depth0(rest, "to") {
        Some((to_start, to_end)) => {
            let t = parse_contrast_target(rest[to_end..].trim())?;
            (&rest[..to_start], Some(t))
        }
        None => (rest, None),
    };

    let candidates: Vec<Color> = split_top_level_commas(list_str)
        .iter()
        .map(|s| parse_color(s.trim()))
        .collect::<Option<Vec<_>>>()?;
    if candidates.len() < 2 {
        return None;
    }

    let best = |cands: &[Color]| -> Option<Color> {
        cands
            .iter()
            .copied()
            .max_by(|a, b| {
                wcag_contrast_ratio(base, *a)
                    .partial_cmp(&wcag_contrast_ratio(base, *b))
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
    };

    match target {
        None => best(&candidates),
        Some(t) => candidates
            .iter()
            .copied()
            .find(|c| wcag_contrast_ratio(base, *c) >= t)
            .or_else(|| best(&candidates)),
    }
}

/// Parses a `<contrast-target>` (CSS Color L5 §11): a WCAG level keyword or a
/// bare numeric contrast ratio. Keyword ratios follow WCAG 2.1 §1.4.3/§1.4.6.
fn parse_contrast_target(s: &str) -> Option<f32> {
    match s.to_ascii_lowercase().as_str() {
        "aa" => Some(4.5),
        "aa-large" => Some(3.0),
        "aaa" => Some(7.0),
        "aaa-large" => Some(4.5),
        other => other.parse::<f32>().ok(),
    }
}

/// Finds a standalone ASCII keyword (`vs` / `to`) at parenthesis depth 0 and
/// returns its `(start, end)` byte range. Word boundaries are honoured — only a
/// complete alphabetic token equal to `kw` matches, so substrings inside color
/// names (e.g. `to` in `tomato`) or function names are skipped, and keywords
/// nested inside `rgb(…)`/`color-mix(…)` arguments are ignored.
fn find_keyword_at_depth0(s: &str, kw: &str) -> Option<(usize, usize)> {
    let bytes = s.as_bytes();
    let mut depth: i32 = 0;
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i];
        if c == b'(' {
            depth += 1;
            i += 1;
        } else if c == b')' {
            depth -= 1;
            i += 1;
        } else if depth == 0 && c.is_ascii_alphabetic() {
            let start = i;
            while i < bytes.len() && bytes[i].is_ascii_alphabetic() {
                i += 1;
            }
            if s[start..i].eq_ignore_ascii_case(kw) {
                return Some((start, i));
            }
        } else {
            i += 1;
        }
    }
    None
}

/// WCAG 2.1 relative luminance of an sRGB color (alpha ignored). Channels are
/// linearised per the WCAG formula before the 0.2126/0.7152/0.0722 weighting.
fn wcag_relative_luminance(c: Color) -> f32 {
    fn linearise(channel: u8) -> f32 {
        let s = channel as f32 / 255.0;
        if s <= 0.03928 {
            s / 12.92
        } else {
            ((s + 0.055) / 1.055).powf(2.4)
        }
    }
    0.2126 * linearise(c.r) + 0.7152 * linearise(c.g) + 0.0722 * linearise(c.b)
}

/// WCAG 2.1 contrast ratio between two colors, in `1.0..=21.0`
/// (`(L_lighter + 0.05) / (L_darker + 0.05)`).
fn wcag_contrast_ratio(a: Color, b: Color) -> f32 {
    let la = wcag_relative_luminance(a);
    let lb = wcag_relative_luminance(b);
    let (hi, lo) = if la >= lb { (la, lb) } else { (lb, la) };
    (hi + 0.05) / (lo + 0.05)
}

/// Parse `"<color> [N%]?"` → `(Color, Option<fraction>)`.
/// Fraction = percentage / 100. Returns `None` only when color itself is invalid.
fn parse_color_with_pct(s: &str) -> Option<(Color, Option<f32>)> {
    // Check if the last whitespace-separated token looks like "N%".
    if let Some(sp_pos) = s.rfind(char::is_whitespace) {
        let last = s[sp_pos + 1..].trim();
        if let Some(digits) = last.strip_suffix('%')
            && let Ok(v) = digits.parse::<f32>()
        {
            let color_str = s[..sp_pos].trim();
            return Some((parse_color(color_str)?, Some(v / 100.0)));
        }
    }
    // No percentage suffix; entire string is the color.
    Some((parse_color(s)?, None))
}

/// Разбивает тело функции по запятой или whitespace (CSS4 разрешает оба),
/// плюс по `/` для отделения alpha в новом синтаксисе `rgb(255 0 0 / 0.5)`.
fn split_color_args(body: &str) -> Vec<String> {
    // Если есть запятые — режем по ним (legacy CSS3).
    if body.contains(',') {
        return body.split(',').map(|s| s.trim().to_string()).collect();
    }
    // Modern CSS4: `r g b` или `r g b / a`. Слэш отделяет alpha.
    let normalized = body.replace('/', " / ");
    let tokens: Vec<&str> = normalized.split_whitespace().collect();
    // Ищем `/` — разделитель alpha.
    if let Some(slash) = tokens.iter().position(|&t| t == "/") {
        let mut head: Vec<String> = tokens[..slash].iter().map(|t| t.to_string()).collect();
        if let Some(alpha) = tokens.get(slash + 1) {
            head.push((*alpha).to_string());
        }
        head
    } else {
        tokens.iter().map(|t| t.to_string()).collect()
    }
}

fn parse_rgb_component(s: &str) -> Option<u8> {
    let s = s.trim();
    if let Some(pct) = s.strip_suffix('%') {
        let p = pct.trim().parse::<f32>().ok()?;
        return Some(clamp_byte((p / 100.0) * 255.0));
    }
    let n = s.parse::<f32>().ok()?;
    Some(clamp_byte(n))
}

fn parse_alpha_component(s: &str) -> Option<u8> {
    let s = s.trim();
    if let Some(pct) = s.strip_suffix('%') {
        let p = pct.trim().parse::<f32>().ok()?;
        return Some(clamp_byte((p / 100.0) * 255.0));
    }
    let n = s.parse::<f32>().ok()?;
    Some(clamp_byte(n * 255.0))
}

/// Парсит hue в градусах. Поддерживает четыре единицы CSS Color L4 §9:
///   - `deg` или без единицы — градусы (default);
///   - `turn` — оборот (1turn = 360deg, как `<a href>` в Кубе Рубика);
///   - `rad` — радианы (1rad = 180/π deg ≈ 57.296deg);
///   - `grad` — гоны (1grad = 0.9deg, full turn = 400grad).
///
/// Порядок проверки суффиксов важен: более длинные сначала, иначе
/// `grad` будет ошибочно ловиться как `rad`.
fn parse_hue_component(s: &str) -> Option<f32> {
    let s = s.trim();
    if let Some(num) = s.strip_suffix("turn") {
        return num.trim().parse::<f32>().ok().map(|v| v * 360.0);
    }
    if let Some(num) = s.strip_suffix("grad") {
        return num.trim().parse::<f32>().ok().map(|v| v * 0.9);
    }
    if let Some(num) = s.strip_suffix("rad") {
        return num.trim().parse::<f32>().ok().map(|v| v * (180.0 / std::f32::consts::PI));
    }
    let s = s.strip_suffix("deg").unwrap_or(s);
    s.trim().parse::<f32>().ok()
}

fn parse_percent_component(s: &str) -> Option<f32> {
    let s = s.trim();
    let pct = s.strip_suffix('%')?;
    let p = pct.trim().parse::<f32>().ok()?;
    Some((p / 100.0).clamp(0.0, 1.0))
}

fn clamp_byte(v: f32) -> u8 {
    v.clamp(0.0, 255.0).round() as u8
}

/// Преобразование HSL → RGB по CSS Color Module Level 3 (как у whatwg).
/// `h` — в градусах (любое значение, нормализуется по mod 360),
/// `s` и `l` — нормированные 0..1.
fn hsl_to_rgb(h: f32, s: f32, l: f32) -> (u8, u8, u8) {
    let h = h.rem_euclid(360.0) / 360.0;
    if s == 0.0 {
        let v = clamp_byte(l * 255.0);
        return (v, v, v);
    }
    let q = if l < 0.5 { l * (1.0 + s) } else { l + s - l * s };
    let p = 2.0 * l - q;
    let r = hue_to_rgb(p, q, h + 1.0 / 3.0);
    let g = hue_to_rgb(p, q, h);
    let b = hue_to_rgb(p, q, h - 1.0 / 3.0);
    (clamp_byte(r * 255.0), clamp_byte(g * 255.0), clamp_byte(b * 255.0))
}

fn hue_to_rgb(p: f32, q: f32, t: f32) -> f32 {
    let t = t.rem_euclid(1.0);
    if t < 1.0 / 6.0 {
        p + (q - p) * 6.0 * t
    } else if t < 0.5 {
        q
    } else if t < 2.0 / 3.0 {
        p + (q - p) * (2.0 / 3.0 - t) * 6.0
    } else {
        p
    }
}
