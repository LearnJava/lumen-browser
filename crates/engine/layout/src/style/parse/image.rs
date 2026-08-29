//! Разбор значений `<image>` (CSS Images L3/L4): `url()`, `image-set()`,
//! `cross-fade()`, `paint()`, слои `background` и `mask`
//! (CSS Backgrounds L3 §3, CSS Masking L1 §4) и градиенты — линейные,
//! радиальные и конические, вместе с их цветовыми остановками.
//!
//! Перенесено батчем SPLIT-ST5 из `crates/engine/layout/src/style.rs`
//! (анкеры `fn parse_url_value` … `fn parse_single_mask_layer`, `fn paren_whitespace_tokens` … `fn parse_conic_stop_position`) без правок тел: изменены только пути модулей и
//! видимость тех items, которые продолжают звать `style.rs`, его тест-модули
//! и соседние модули `style::parse`.

use lumen_core::geom::Size;

use crate::color_mix::{HueInterpolationMethod, MixColorSpace, mix_colors_hue};
use crate::style::parse::box_sides::resolve_box_length;
use crate::style::parse::color::{parse_color, parse_css_color_legacy};
use crate::style::parse::transform::parse_angle_to_radians;
use crate::style::{
    BackgroundAttachment, BackgroundClip, BackgroundImage, BackgroundLayer, BackgroundOrigin,
    BackgroundRepeat, BackgroundSize, BgSizeAxis, Color, ComputedStyle, CssColor, GradientCorner,
    GradientStop, Length, MaskClip, MaskComposite, MaskLayer, MaskMode, ObjectPosition,
    ParsedGradient, RadialShape, RadialSize, parse_length_q, split_top_level_commas,
};

/// Извлечь URL из `url(...)`-функции. Поддерживает кавычки и без них.
/// Возвращает None если строка не выглядит как url().
pub(in crate::style) fn parse_url_value(s: &str) -> Option<String> {
    let s = s.trim();
    let after = s.strip_prefix("url(")?;
    let close = after.rfind(')')?;
    let inner = after[..close].trim().trim_matches(['"', '\''].as_ref());
    Some(inner.to_string())
}

/// Проверка, является ли value одной из gradient-функций.
pub(in crate::style) fn is_gradient_function(s: &str) -> bool {
    let s = s.trim().to_ascii_lowercase();
    s.starts_with("linear-gradient(")
        || s.starts_with("radial-gradient(")
        || s.starts_with("conic-gradient(")
        || s.starts_with("repeating-linear-gradient(")
        || s.starts_with("repeating-radial-gradient(")
        || s.starts_with("repeating-conic-gradient(")
}

/// CSS Images L4 §5 — is `s` an `image-set()` / `-webkit-image-set()` expression?
pub(in crate::style) fn is_image_set_value(s: &str) -> bool {
    crate::image_set::is_image_set(s)
}

/// CSS Paint API (Houdini) — parse `paint(name)` and extract the worklet name.
/// Returns `Some(name)` if the function is recognized; `None` otherwise.
pub(in crate::style) fn parse_paint_function(s: &str) -> Option<String> {
    let s = s.trim();
    let lower = s.to_ascii_lowercase();
    if lower.starts_with("paint(") && s.ends_with(')') && !lower.ends_with("))") {
        // Extract content between "paint(" and the final ")".
        let start = "paint(".len();
        let end = s.len() - 1;
        if start >= end {
            return None;  // Empty payload.
        }
        let inner = s[start..end].trim();
        // Check for any stray closing parens in the extracted part.
        if inner.contains(')') {
            return None;
        }
        // Remove surrounding quotes if present (e.g., `paint("my-paint")` → `my-paint`).
        let name = if (inner.starts_with('"') && inner.ends_with('"')) ||
                     (inner.starts_with('\'') && inner.ends_with('\'')) {
            if inner.len() < 2 {
                return None;  // Quote-only string.
            }
            &inner[1..inner.len() - 1]
        } else {
            inner
        };
        if name.is_empty() {
            return None;  // Empty name.
        }
        return Some(name.to_string());
    }
    None
}

/// Parse one image value into a `BackgroundImage` (used by `parse_cross_fade`).
fn parse_bg_image_value(s: &str) -> Option<BackgroundImage> {
    let s = s.trim();
    if s.eq_ignore_ascii_case("none") {
        return Some(BackgroundImage::None);
    }
    if is_image_set_value(s) {
        return Some(BackgroundImage::Url(s.to_string()));
    }
    if let Some(url) = parse_url_value(s) {
        return Some(BackgroundImage::Url(url));
    }
    if is_gradient_function(s) {
        return Some(BackgroundImage::Gradient(parse_background_gradient(s)));
    }
    None
}

/// Parse a `cross-fade()` / `-webkit-cross-fade()` function into the two-image
/// blend `(image-a, image-b, t)` where `t ∈ 0.0..=1.0` is the fraction occupied
/// by `image-b` (`0.0` = only `image-a`, `1.0` = only `image-b`).
///
/// Two grammars are accepted, selected by the vendor prefix:
///
/// * **`-webkit-cross-fade(<from>, <to>, <percentage>)`** — the legacy
///   three-argument form. `<percentage>` is the blend progress from `<from>`
///   (`0%`) to `<to>` (`100%`). Kept for content that targets the prefixed
///   syntax.
/// * **`cross-fade( [<percentage>? && <image>]# )`** — the standard
///   CSS Images L4 §4 form. Only the common two-image case is supported; each
///   argument is an `<image>` with an optional opacity `<percentage>` in either
///   order. A bare percentage with no image is invalid.
///
/// The legacy three-argument `cross-fade(<from>, <to>, <percentage>)` **without**
/// the `-webkit-` prefix is rejected (returns `None`) — the trailing bare
/// `<percentage>` is not a valid `<image>`. This matches reference browsers
/// (Edge/Chromium drop the declaration), so the caller renders nothing.
///
/// Returns `None` when `s` is not a cross-fade function or violates the grammar
/// for its prefix; the declaration is then dropped by the caller.
pub(in crate::style) fn parse_cross_fade(s: &str) -> Option<(BackgroundImage, BackgroundImage, f32)> {
    let s = s.trim();
    if !s.ends_with(')') {
        return None;
    }
    let lower = s.to_ascii_lowercase();
    if lower.starts_with("-webkit-cross-fade(") {
        let inner = &s["-webkit-cross-fade(".len()..s.len() - 1];
        return parse_webkit_cross_fade(inner);
    }
    if lower.starts_with("cross-fade(") {
        let inner = &s["cross-fade(".len()..s.len() - 1];
        return parse_l4_cross_fade(inner);
    }
    None
}

/// Legacy `-webkit-cross-fade(<from>, <to>, <percentage>)` — exactly three
/// arguments; `<percentage>` is the blend progress toward `<to>`.
fn parse_webkit_cross_fade(inner: &str) -> Option<(BackgroundImage, BackgroundImage, f32)> {
    let parts = split_top_level_commas(inner);
    if parts.len() != 3 {
        return None;
    }
    let a = parse_bg_image_value(parts[0].trim())?;
    let b = parse_bg_image_value(parts[1].trim())?;
    let t = parse_cf_percentage(parts[2].trim())?;
    Some((a, b, t.clamp(0.0, 1.0)))
}

/// Standard L4 `cross-fade( [<percentage>? && <image>]# )` — two-image form.
///
/// Each argument carries an optional opacity percentage. The returned `t` is the
/// fraction of `image-b`: an image's opacity is its declared percentage, and an
/// image without one takes the remaining weight.
fn parse_l4_cross_fade(inner: &str) -> Option<(BackgroundImage, BackgroundImage, f32)> {
    let parts = split_top_level_commas(inner);
    if parts.len() != 2 {
        return None;
    }
    let (img_a, pct_a) = parse_cf_image(parts[0].trim())?;
    let (img_b, pct_b) = parse_cf_image(parts[1].trim())?;
    let t = match (pct_a, pct_b) {
        (None, None) => 0.5,
        (Some(pa), None) => 1.0 - pa,
        (None, Some(pb)) => pb,
        (Some(pa), Some(pb)) => {
            let sum = pa + pb;
            if sum > 0.0 { pb / sum } else { 0.5 }
        }
    };
    Some((img_a, img_b, t.clamp(0.0, 1.0)))
}

/// Parse one L4 `<cf-image>` = `<percentage>? && <image>` (tokens in any order).
///
/// Returns the image plus its optional opacity fraction (`0.0..=1.0`). `None`
/// when there is no `<image>` (e.g. a bare percentage) or the tokens don't form
/// a valid image-plus-optional-percentage pair.
fn parse_cf_image(part: &str) -> Option<(BackgroundImage, Option<f32>)> {
    let tokens = split_top_level_ws(part);
    match tokens.as_slice() {
        [tok] => {
            // A lone token must be an image; a bare percentage is invalid.
            Some((parse_bg_image_value(tok)?, None))
        }
        [a, b] => {
            // `<percentage> <image>` or `<image> <percentage>`.
            if let Some(p) = parse_cf_percentage(a) {
                Some((parse_bg_image_value(b)?, Some(p.clamp(0.0, 1.0))))
            } else if let Some(p) = parse_cf_percentage(b) {
                Some((parse_bg_image_value(a)?, Some(p.clamp(0.0, 1.0))))
            } else {
                None
            }
        }
        _ => None,
    }
}

/// Parse a cross-fade percentage/number argument into a `0.0..=1.0` fraction.
/// `50%` → `0.5`, `0.5` → `0.5`. `None` if the token is not a number.
fn parse_cf_percentage(s: &str) -> Option<f32> {
    let s = s.trim();
    if let Some(pct) = s.strip_suffix('%') {
        Some(pct.trim().parse::<f32>().ok()? / 100.0)
    } else {
        s.parse::<f32>().ok()
    }
}

/// Split `s` on top-level ASCII whitespace, keeping parenthesised groups
/// (e.g. `url(a b)`, `linear-gradient(…)`) intact. Empty tokens are dropped.
fn split_top_level_ws(s: &str) -> Vec<&str> {
    let bytes = s.as_bytes();
    let mut tokens = Vec::new();
    let mut start = 0;
    let mut depth = 0usize;
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'(' => depth += 1,
            b')' => depth = depth.saturating_sub(1),
            b' ' | b'\t' | b'\n' | b'\r' if depth == 0 => {
                let tok = s[start..i].trim();
                if !tok.is_empty() {
                    tokens.push(tok);
                }
                start = i + 1;
            }
            _ => {}
        }
        i += 1;
    }
    let tok = s[start..].trim();
    if !tok.is_empty() {
        tokens.push(tok);
    }
    tokens
}

/// CSS Backgrounds L3 §3 — разбить строку одного слоя на токены.
///
/// Делит по пробелам и `/` только на depth=0 (не трогает содержимое функций
/// вроде `url(path/img.png)` или `linear-gradient(red, blue)`).
fn tokenize_bg_layer(s: &str) -> Vec<&str> {
    let mut tokens = Vec::new();
    let bytes = s.as_bytes();
    let mut start = 0;
    let mut depth = 0usize;
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'(' => { depth += 1; i += 1; }
            b')' => { depth = depth.saturating_sub(1); i += 1; }
            b' ' | b'\t' | b'\n' | b'\r' if depth == 0 => {
                let tok = s[start..i].trim();
                if !tok.is_empty() { tokens.push(tok); }
                start = i + 1;
                i += 1;
            }
            b'/' if depth == 0 => {
                let tok = s[start..i].trim();
                if !tok.is_empty() { tokens.push(tok); }
                tokens.push("/");
                start = i + 1;
                i += 1;
            }
            _ => { i += 1; }
        }
    }
    if start < bytes.len() {
        let tok = s[start..].trim();
        if !tok.is_empty() { tokens.push(tok); }
    }
    tokens
}

/// Возвращает `true` если токен может быть значением `background-position`.
fn is_bg_position_token(s: &str) -> bool {
    let lo = s.to_ascii_lowercase();
    matches!(lo.as_str(), "center" | "top" | "bottom" | "left" | "right")
        || s.ends_with('%')
        || s.ends_with("px")
        || s.ends_with("em")
        || s.ends_with("rem")
        || s.ends_with("vw")
        || s.ends_with("vh")
        || s.ends_with("vmin")
        || s.ends_with("vmax")
        || s.parse::<f32>().is_ok_and(|v| v == 0.0)
}

/// CSS Backgrounds L3 §3.5 — parse one axis token of `background-size` /
/// `mask-size`: `auto` → `Auto`, `<percentage>` → `Percent` (fraction),
/// `<length>` → `Px`. Returns `None` if the token isn't a valid axis value.
fn parse_bg_size_axis(s: &str, em_basis: f32, viewport: Size, is_quirks: bool) -> Option<BgSizeAxis> {
    let s = s.trim();
    if s.eq_ignore_ascii_case("auto") {
        return Some(BgSizeAxis::Auto);
    }
    if let Some(pct) = s.strip_suffix('%') {
        return pct.trim().parse::<f32>().ok().map(|v| BgSizeAxis::Percent(v / 100.0));
    }
    resolve_box_length(s, em_basis, viewport, is_quirks).map(BgSizeAxis::Px)
}

/// CSS Backgrounds L3 §3.5 — parse background-size от одного токена
/// (`auto` / `cover` / `contain` / длина / процент). Height axis = `Auto`;
/// the caller may combine a following token for the second axis.
fn parse_background_size_single(s: &str, em_basis: f32, viewport: Size, is_quirks: bool) -> BackgroundSize {
    let t = s.trim();
    if t.eq_ignore_ascii_case("cover") {
        return BackgroundSize::Cover;
    }
    if t.eq_ignore_ascii_case("contain") {
        return BackgroundSize::Contain;
    }
    match parse_bg_size_axis(t, em_basis, viewport, is_quirks) {
        Some(BgSizeAxis::Auto) | None => BackgroundSize::Auto,
        Some(w) => BackgroundSize::Length(w, BgSizeAxis::Auto),
    }
}

/// CSS Backgrounds L3 §3.5 — parse a single-layer `background-size` / `mask-size`
/// value: `cover` / `contain` / one or two `<length-percentage> | auto` axes.
pub(in crate::style) fn parse_background_size_value(s: &str, em_basis: f32, viewport: Size, is_quirks: bool) -> BackgroundSize {
    let t = s.trim();
    if t.eq_ignore_ascii_case("cover") {
        return BackgroundSize::Cover;
    }
    if t.eq_ignore_ascii_case("contain") {
        return BackgroundSize::Contain;
    }
    let parts: Vec<&str> = t.split_whitespace().collect();
    let Some(w) = parts.first().and_then(|p| parse_bg_size_axis(p, em_basis, viewport, is_quirks))
    else {
        return BackgroundSize::Auto;
    };
    let h = parts.get(1)
        .and_then(|p| parse_bg_size_axis(p, em_basis, viewport, is_quirks))
        .unwrap_or(BgSizeAxis::Auto);
    if w == BgSizeAxis::Auto && h == BgSizeAxis::Auto {
        BackgroundSize::Auto
    } else {
        BackgroundSize::Length(w, h)
    }
}

/// CSS Backgrounds L3 §3 — parse single layer of `background` shorthand.
///
/// Принимает строку одного слоя (после разбивки по top-level `,`).
/// Возвращает `(BackgroundLayer, Option<CssColor>)` — цвет есть только
/// у последнего слоя, но мы не знаем последний ли этот слой, поэтому
/// caller сам решает, использовать ли цвет.
pub(in crate::style) fn parse_single_bg_layer(
    layer_str: &str,
    em_basis: f32,
    viewport: Size,
    is_quirks: bool,
) -> (BackgroundLayer, Option<CssColor>) {
    let mut layer = BackgroundLayer::default();
    let mut color: Option<CssColor> = None;
    let tokens = tokenize_bg_layer(layer_str);
    let n = tokens.len();
    let mut idx = 0;

    while idx < n {
        let t = tokens[idx];

        // image: none / url(...) / gradient(...) / paint(...) / cross-fade(...)
        if t.eq_ignore_ascii_case("none") && layer.image == BackgroundImage::None {
            // "none" as image — keep None (default already)
            idx += 1;
            continue;
        }
        if is_gradient_function(t) {
            layer.image = BackgroundImage::Gradient(parse_background_gradient(t));
            idx += 1;
            continue;
        }
        if let Some(name) = parse_paint_function(t) {
            // CSS Paint API (Houdini) — `paint(name)` → fetch registered worklet.
            // Phase 0: stores as Paint(name); Phase 1: invokes worklet paint() callback.
            // `// CSS: background: paint(name)`
            layer.image = BackgroundImage::Paint(name);
            idx += 1;
            continue;
        }
        if is_image_set_value(t) {
            // Store raw image-set(…) string; paint resolves per-DPR (CSS Images L4 §5).
            layer.image = BackgroundImage::Url(t.to_string());
            idx += 1;
            continue;
        }
        if let Some((a, b, cf_t)) = parse_cross_fade(t) {
            layer.image = BackgroundImage::CrossFade { a: Box::new(a), b: Box::new(b), t: cf_t };
            idx += 1;
            continue;
        }
        if t.to_ascii_lowercase().starts_with("url(") {
            if let Some(url) = parse_url_value(t) {
                layer.image = BackgroundImage::Url(url);
            }
            idx += 1;
            continue;
        }

        // background-repeat keywords
        if let Some(r) = BackgroundRepeat::parse(t) {
            layer.repeat = r;
            idx += 1;
            continue;
        }

        // background-attachment keywords
        if let Some(a) = BackgroundAttachment::parse(t) {
            layer.attachment = a;
            idx += 1;
            continue;
        }

        // box keywords: border-box | padding-box | content-box
        // First occurrence = origin AND clip; second occurrence = clip only.
        if let Some(o) = BackgroundOrigin::parse(t) {
            // Первое box-keyword: origin = this; если дальше ещё keyword, оно = clip
            layer.origin = o;
            // По умолчанию clip совпадает с origin (CSS spec §3.6 initial handling)
            layer.clip = match o {
                BackgroundOrigin::BorderBox => BackgroundClip::BorderBox,
                BackgroundOrigin::PaddingBox => BackgroundClip::PaddingBox,
                BackgroundOrigin::ContentBox => BackgroundClip::ContentBox,
            };
            // Проверим следующий токен: может быть вторым box-keyword для clip
            if idx + 1 < n && let Some(c2) = BackgroundClip::parse(tokens[idx + 1]) {
                layer.clip = c2;
                idx += 2;
                continue;
            }
            idx += 1;
            continue;
        }
        // background-clip only (text)
        if t.eq_ignore_ascii_case("text") {
            layer.clip = BackgroundClip::Text;
            idx += 1;
            continue;
        }

        // position: one or two position-tokens, optionally followed by / size
        if t != "/" && is_bg_position_token(t) {
            let mut pos_parts = vec![t];
            // Второй позиционный токен?
            if idx + 1 < n && tokens[idx + 1] != "/" && is_bg_position_token(tokens[idx + 1]) {
                pos_parts.push(tokens[idx + 1]);
                idx += 1;
            }
            let pos_str = pos_parts.join(" ");
            if let Some(p) = ObjectPosition::parse(&pos_str, em_basis, viewport) {
                layer.position = p;
            }
            idx += 1;

            // После позиции может идти `/` и затем size
            if idx < n && tokens[idx] == "/" {
                idx += 1; // пропустить /
                if idx < n {
                    let s1 = tokens[idx];
                    let mut size = parse_background_size_single(s1, em_basis, viewport, is_quirks);
                    idx += 1;
                    // Второй токен для size (width height)?
                    if idx < n && tokens[idx] != "/" && !is_gradient_function(tokens[idx])
                        && BackgroundRepeat::parse(tokens[idx]).is_none()
                        && BackgroundAttachment::parse(tokens[idx]).is_none()
                        && BackgroundOrigin::parse(tokens[idx]).is_none()
                    {
                        if let BackgroundSize::Length(w, BgSizeAxis::Auto) = size {
                            if let Some(h) = parse_bg_size_axis(tokens[idx], em_basis, viewport, is_quirks) {
                                size = BackgroundSize::Length(w, h);
                                idx += 1;
                            }
                        } else if matches!(size, BackgroundSize::Auto) {
                            // `auto <axis>` → width auto, height = axis (intrinsic-ratio width).
                            if let Some(h) = parse_bg_size_axis(tokens[idx], em_basis, viewport, is_quirks) {
                                if h != BgSizeAxis::Auto {
                                    size = BackgroundSize::Length(BgSizeAxis::Auto, h);
                                }
                                idx += 1;
                            }
                        }
                    }
                    layer.size = size;
                }
            }
            continue;
        }

        // color — пробуем последним, чтобы не спутать с keywords.
        // BUG-079: hashless-hex quirk (Quirks Mode §3.4) применяется ТОЛЬКО к
        // лонгхендам `background-color` / `color` / `border-*-color`, но НЕ к
        // шортхенду `background`. Поэтому здесь quirks-флаг всегда `false`:
        // `background: ff4444` в quirks-mode невалиден (Edge не красит), хотя
        // `background-color: ff4444` — валиден.
        if let Some(c) = parse_css_color_legacy(t, false) {
            color = Some(c);
            idx += 1;
            continue;
        }

        idx += 1; // неизвестный токен — пропустить
    }

    (layer, color)
}

/// CSS Masking L1 §4.9 — раскладывает значения одного mask-longhand-а по
/// слоям [`ComputedStyle::mask_layers`] с циклическим повторением.
///
/// Количество слоёв задаёт `mask-image`. Если слоёв ещё нет (longhand объявлен
/// без `mask-image` или раньше него в том же блоке), создаётся один слой с
/// initial-значениями — иначе объявление молча потерялось бы; тот же приём, что
/// у `background-*`. Пустой `values` (все элементы невалидны) — no-op:
/// невалидное объявление не должно затирать уже применённое.
pub(in crate::style) fn apply_mask_longhand<T: Copy>(
    style: &mut ComputedStyle,
    values: &[T],
    set: impl Fn(&mut MaskLayer, T),
) {
    if values.is_empty() {
        return;
    }
    if style.mask_layers.is_empty() {
        style.mask_layers.push(MaskLayer::default());
    }
    let n = values.len();
    for (i, layer) in style.mask_layers.iter_mut().enumerate() {
        set(layer, values[i % n]);
    }
}

/// CSS Masking L1 §4.8 — разбирает один слой шортхенда `mask`.
///
/// Компоненты идут в любом порядке (`||` в грамматике), поэтому каждый токен
/// классифицируется по своему множеству ключевых слов. Незаданные компоненты
/// остаются initial-значениями [`MaskLayer::default`] — это и есть reset-часть
/// семантики шортхенда.
///
/// `<geometry-box>` заполняет два независимых слота — origin и clip — а не
/// «первое вхождение / второе вхождение» подряд: `no-clip` может занять слот
/// clip только, поэтому `no-clip padding-box` даёт `origin: padding-box` +
/// `clip: no-clip`, а не `clip: padding-box`. Одиночный `<geometry-box>`
/// задаёт **оба** слота (§4.8, как у `background`); при двух — первый идёт в
/// origin, второй в clip.
///
/// Ключевые слова `fill-box` / `stroke-box` / `view-box` в позиции origin
/// схлопываются до `border-box`-семантики (`mask-origin` в нашей модели —
/// [`BackgroundOrigin`] из трёх CSS-боксов), но в позиции clip сохраняются
/// точно — это ровно та же аппроксимация, что и у longhand `mask-origin`.
pub(in crate::style) fn parse_single_mask_layer(
    layer_str: &str,
    em_basis: f32,
    viewport: Size,
    is_quirks: bool,
) -> MaskLayer {
    let mut layer = MaskLayer::default();
    let tokens = tokenize_bg_layer(layer_str);
    let n = tokens.len();
    let mut idx = 0;
    // Слоты origin / clip заполняются независимо: `no-clip` занимает только
    // clip, поэтому счётчиком «первый / второй бокс» обойтись нельзя.
    let mut origin_set = false;
    let mut clip_set = false;

    while idx < n {
        let t = tokens[idx];

        // <mask-reference>: none | url(...) | <gradient>
        if t.eq_ignore_ascii_case("none") {
            layer.image = BackgroundImage::None;
            idx += 1;
            continue;
        }
        if is_gradient_function(t) {
            layer.image = BackgroundImage::Gradient(parse_background_gradient(t));
            idx += 1;
            continue;
        }
        if t.to_ascii_lowercase().starts_with("url(") {
            if let Some(url) = parse_url_value(t) {
                layer.image = BackgroundImage::Url(url);
            }
            idx += 1;
            continue;
        }

        // <masking-mode> — до <repeat-style>/<geometry-box>, множества не
        // пересекаются, порядок здесь только для читаемости.
        if t.eq_ignore_ascii_case("luminance") {
            layer.mode = MaskMode::Luminance;
            idx += 1;
            continue;
        }
        if t.eq_ignore_ascii_case("alpha") || t.eq_ignore_ascii_case("match-source") {
            layer.mode = MaskMode::Alpha;
            idx += 1;
            continue;
        }

        // <compositing-operator>
        if let Some(c) = MaskComposite::parse(t) {
            layer.composite = c;
            idx += 1;
            continue;
        }

        // <repeat-style>
        if let Some(r) = BackgroundRepeat::parse(t) {
            layer.repeat = r;
            idx += 1;
            continue;
        }

        // <geometry-box> | no-clip
        if let Some(c) = MaskClip::parse(t) {
            if t.eq_ignore_ascii_case("no-clip") {
                // `no-clip` валиден только как mask-clip — слот origin не трогаем.
                layer.clip = c;
                clip_set = true;
            } else if !origin_set {
                // Первый настоящий <geometry-box> идёт в origin и, пока clip не
                // занят, дублируется в clip (одиночный бокс задаёт оба).
                if let Some(o) = BackgroundOrigin::parse(t) {
                    layer.origin = o;
                }
                origin_set = true;
                if !clip_set {
                    layer.clip = c;
                }
            } else if !clip_set {
                layer.clip = c;
                clip_set = true;
            }
            idx += 1;
            continue;
        }

        // <position> [ / <bg-size> ]?
        if t != "/" && is_bg_position_token(t) {
            let mut pos_parts = vec![t];
            if idx + 1 < n && tokens[idx + 1] != "/" && is_bg_position_token(tokens[idx + 1]) {
                pos_parts.push(tokens[idx + 1]);
                idx += 1;
            }
            if let Some(p) = ObjectPosition::parse(&pos_parts.join(" "), em_basis, viewport) {
                layer.position = p;
            }
            idx += 1;

            if idx < n && tokens[idx] == "/" {
                idx += 1; // пропустить `/`
                if idx < n {
                    let mut size =
                        parse_background_size_single(tokens[idx], em_basis, viewport, is_quirks);
                    idx += 1;
                    // Второй токен size (`<width> <height>`) — только если он не
                    // начало следующей компоненты слоя.
                    if idx < n
                        && tokens[idx] != "/"
                        && !is_gradient_function(tokens[idx])
                        && BackgroundRepeat::parse(tokens[idx]).is_none()
                        && MaskClip::parse(tokens[idx]).is_none()
                        && MaskComposite::parse(tokens[idx]).is_none()
                        && let Some(h) = parse_bg_size_axis(tokens[idx], em_basis, viewport, is_quirks)
                    {
                        match size {
                            BackgroundSize::Length(w, BgSizeAxis::Auto) => {
                                size = BackgroundSize::Length(w, h);
                                idx += 1;
                            }
                            BackgroundSize::Auto => {
                                if h != BgSizeAxis::Auto {
                                    size = BackgroundSize::Length(BgSizeAxis::Auto, h);
                                }
                                idx += 1;
                            }
                            _ => {}
                        }
                    }
                    layer.size = size;
                }
            }
            continue;
        }

        idx += 1; // неизвестный токен — пропустить
    }

    layer
}

/// Whitespace tokenizer that treats `(...)` as an opaque unit.
/// Used to split a gradient color-stop segment into `<color>` and
/// optional `<length-percentage>` parts without breaking color functions
/// like `rgba(0, 128, 0, 0.5)`.
fn paren_whitespace_tokens(s: &str) -> Vec<String> {
    let mut tokens: Vec<String> = Vec::new();
    let mut buf = String::new();
    let mut depth = 0i32;
    for c in s.chars() {
        match c {
            '(' => {
                depth += 1;
                buf.push(c);
            }
            ')' => {
                depth -= 1;
                buf.push(c);
            }
            w if w.is_whitespace() && depth == 0 => {
                let t = buf.trim().to_string();
                if !t.is_empty() {
                    tokens.push(t);
                }
                buf.clear();
            }
            _ => buf.push(c),
        }
    }
    let t = buf.trim().to_string();
    if !t.is_empty() {
        tokens.push(t);
    }
    tokens
}

/// CSS Images L3/L4 §3.3/§3.7 — parses color stops from a CSS gradient string.
///
/// Accepts `linear-gradient(...)`, `radial-gradient(...)`,
/// `conic-gradient(...)` and their `repeating-` variants.
///
/// Parse a CSS gradient function string into a [`ParsedGradient`].
///
/// Recognises `linear-gradient`, `repeating-linear-gradient`,
/// `radial-gradient`, `repeating-radial-gradient`, `conic-gradient`,
/// `repeating-conic-gradient`. Anything else is returned as
/// `ParsedGradient::Unknown`.
pub fn parse_background_gradient(s: &str) -> ParsedGradient {
    let s = s.trim();
    let lower = s.to_ascii_lowercase();

    let repeating_linear = lower.starts_with("repeating-linear-gradient");
    let repeating_radial = lower.starts_with("repeating-radial-gradient");
    let repeating_conic = lower.starts_with("repeating-conic-gradient");
    let is_linear = lower.starts_with("linear-gradient") || repeating_linear;
    let is_radial = lower.starts_with("radial-gradient") || repeating_radial;
    let is_conic = lower.starts_with("conic-gradient") || repeating_conic;

    if !is_linear && !is_radial && !is_conic {
        return ParsedGradient::Unknown(s.to_string());
    }

    // Extract the argument string inside the outermost parens.
    let Some(open) = s.find('(') else {
        return ParsedGradient::Unknown(s.to_string());
    };
    let rest = &s[open + 1..];
    let Some(close) = rest.rfind(')') else {
        return ParsedGradient::Unknown(s.to_string());
    };
    let inner = rest[..close].trim();

    let segments = split_top_level_commas(inner);

    // CSS Images L4 §3.1 — the prelude (first comma-segment) may carry a
    // `<color-interpolation-method>` (`in <space> [<hue> hue]?`) in any order
    // with the direction/shape. Strip it so the direction parsers see a clean
    // prelude, and apply the resulting space to the stop list.
    let first_seg = segments.first().map(|s| s.trim()).unwrap_or("");
    let (clean_first, interp_space, hue_method) = extract_gradient_interpolation(first_seg);

    let interp = |stops: Vec<GradientStop>| -> Vec<GradientStop> {
        match interp_space {
            Some(sp) if sp != MixColorSpace::Srgb => {
                densify_gradient_stops_for_space(&stops, sp, hue_method)
            }
            _ => stops,
        }
    };

    if is_linear {
        // The first segment may be an angle / "to <side>" direction.
        let (angle_deg, corner) = parse_linear_gradient_angle(&clean_first);
        let stops = interp(parse_gradient_stops(s));
        ParsedGradient::Linear { angle_deg, corner, stops, repeating: repeating_linear }
    } else if is_radial {
        // Radial: look for `[<shape> || <size>]? [at <x> <y>]?` in the first segment.
        let (cx, cy) = parse_radial_gradient_center(&clean_first);
        let (shape, size) = parse_radial_gradient_shape_size(&clean_first);
        let stops = interp(parse_gradient_stops(s));
        ParsedGradient::Radial {
            center_x_pct: cx,
            center_y_pct: cy,
            shape,
            size,
            stops,
            repeating: repeating_radial,
        }
    } else {
        // Conic: `[from <angle>]? [at <x> <y>]?` in the first segment.
        let (from_angle_deg, cx, cy) = parse_conic_gradient_params(&clean_first);
        let stops = interp(parse_gradient_stops(s));
        ParsedGradient::Conic {
            center_x_pct: cx,
            center_y_pct: cy,
            from_angle_deg,
            stops,
            repeating: repeating_conic,
        }
    }
}

/// CSS Images L4 §3.1 — parse and strip the `<color-interpolation-method>`
/// (`in <space> [<hue> hue]?`) from a gradient prelude segment.
///
/// Returns the prelude with the clause removed, the parsed interpolation space
/// (`None` if absent), and the `<hue-interpolation-method>` (defaults to
/// [`HueInterpolationMethod::Shorter`], the CSS default, when absent or for
/// non-polar spaces).
///
/// Tokens that are not part of the interpolation clause (direction, `to <side>`,
/// `circle`/`ellipse`, `from <angle>`, `at <x> <y>`) are preserved in order, so
/// `linear-gradient(45deg in oklch, …)` and `linear-gradient(in oklch 45deg, …)`
/// both yield `("45deg", Some(Oklch), Shorter)`, while
/// `linear-gradient(in oklch longer hue, …)` yields `Longer`.
pub(in crate::style) fn extract_gradient_interpolation(
    prelude: &str,
) -> (String, Option<MixColorSpace>, HueInterpolationMethod) {
    let tokens: Vec<&str> = prelude.split_whitespace().collect();
    let mut space = None;
    let mut hue = HueInterpolationMethod::Shorter;
    let mut kept: Vec<&str> = Vec::with_capacity(tokens.len());
    let mut i = 0;
    while i < tokens.len() {
        if space.is_none()
            && tokens[i].eq_ignore_ascii_case("in")
            && let Some(sp) = tokens.get(i + 1).and_then(|t| MixColorSpace::from_css(t))
        {
            space = Some(sp);
            i += 2;
            // Optional `<hue-interpolation-method> hue` for polar spaces.
            if let (Some(h), Some(hue_kw)) = (tokens.get(i), tokens.get(i + 1))
                && let Some(m) = HueInterpolationMethod::from_css(h)
                && hue_kw.eq_ignore_ascii_case("hue")
            {
                hue = m;
                i += 2;
            }
            continue;
        }
        kept.push(tokens[i]);
        i += 1;
    }
    (kept.join(" "), space, hue)
}

/// CSS Images L4 §3.1 — approximate gradient color interpolation in a non-sRGB
/// `space` by subdividing every adjacent stop pair into intermediate stops whose
/// colors are computed via [`mix_colors`](crate::color_mix::mix_colors) in that
/// space. The renderer then interpolates the dense stop list linearly in sRGB,
/// which closely matches true interpolation in `space` (e.g. `in oklch` keeps
/// red→blue vivid instead of the muddy grey-purple of sRGB).
///
/// Stop positions are first resolved to percentages per CSS Images §3.4.3
/// (first→0%, last→100%, interior runs evenly distributed, monotonic clamp).
/// Returns the stops unchanged when there are fewer than two, or when any stop
/// uses a non-percentage (`px`) position — px positions need the gradient line
/// length, which is unknown at parse time.
///
/// `hue` selects the `<hue-interpolation-method>` (CSS Color L4 §12.4) for polar
/// spaces; non-polar spaces ignore it.
fn densify_gradient_stops_for_space(
    stops: &[GradientStop],
    space: MixColorSpace,
    hue: HueInterpolationMethod,
) -> Vec<GradientStop> {
    if stops.len() < 2 {
        return stops.to_vec();
    }
    // Resolve positions to percentages [0, 100]; bail on any non-percentage.
    let mut pos: Vec<Option<f32>> = Vec::with_capacity(stops.len());
    for st in stops {
        match &st.position {
            None => pos.push(None),
            Some(Length::Percent(p)) => pos.push(Some(*p)),
            Some(_) => return stops.to_vec(),
        }
    }
    let last = pos.len() - 1;
    if pos[0].is_none() {
        pos[0] = Some(0.0);
    }
    if pos[last].is_none() {
        pos[last] = Some(100.0);
    }
    // Evenly distribute interior runs of unpositioned stops between anchors.
    let mut i = 1;
    while i < pos.len() {
        if pos[i].is_some() {
            i += 1;
            continue;
        }
        let prev = pos[i - 1].unwrap_or(0.0);
        let mut k = i;
        while k < pos.len() && pos[k].is_none() {
            k += 1;
        }
        let next = pos.get(k).and_then(|p| *p).unwrap_or(100.0);
        let gaps = (k - i + 1) as f32;
        for (offset, idx) in (i..k).enumerate() {
            let frac = (offset + 1) as f32 / gaps;
            pos[idx] = Some(prev + (next - prev) * frac);
        }
        i = k;
    }
    // Enforce monotonic non-decreasing positions.
    let mut resolved: Vec<f32> = pos.into_iter().map(|p| p.unwrap_or(0.0)).collect();
    for n in 1..resolved.len() {
        if resolved[n] < resolved[n - 1] {
            resolved[n] = resolved[n - 1];
        }
    }

    let to_f = |c: Color| -> [f32; 4] {
        [
            c.r as f32 / 255.0,
            c.g as f32 / 255.0,
            c.b as f32 / 255.0,
            c.a as f32 / 255.0,
        ]
    };
    let from_f = |m: [f32; 4]| -> Color {
        Color {
            r: (m[0] * 255.0).round().clamp(0.0, 255.0) as u8,
            g: (m[1] * 255.0).round().clamp(0.0, 255.0) as u8,
            b: (m[2] * 255.0).round().clamp(0.0, 255.0) as u8,
            a: (m[3] * 255.0).round().clamp(0.0, 255.0) as u8,
        }
    };

    // Number of sub-segments per stop pair. 16 keeps the polyfill within the
    // 0.5% diff budget against true space interpolation while bounding output.
    const SEGMENTS: usize = 16;
    let mut out: Vec<GradientStop> = Vec::with_capacity((stops.len() - 1) * SEGMENTS + 1);
    out.push(GradientStop {
        color: stops[0].color,
        color_space: stops[0].color_space,
        position: Some(Length::Percent(resolved[0])),
    });
    for w in 0..stops.len() - 1 {
        let a = to_f(stops[w].color);
        let b = to_f(stops[w + 1].color);
        let p0 = resolved[w];
        let p1 = resolved[w + 1];
        for j in 1..SEGMENTS {
            let t = j as f32 / SEGMENTS as f32;
            out.push(GradientStop {
                color: from_f(mix_colors_hue(space, a, 1.0 - t, b, t, hue)),
                color_space: stops[w].color_space,
                position: Some(Length::Percent(p0 + (p1 - p0) * t)),
            });
        }
        out.push(GradientStop {
            color: stops[w + 1].color,
            color_space: stops[w + 1].color_space,
            position: Some(Length::Percent(p1)),
        });
    }
    out
}

/// Parse the direction/angle portion of a `linear-gradient`.
/// Returns the angle in CSS degrees (0° = to top, 90° = to right,
/// 180° = to bottom, 270° = to left), plus the corner keyword when the
/// direction was a `to <corner>` (the angle is then only a square-box
/// placeholder — see [`GradientCorner::angle_deg`] for the aspect-ratio
/// -correct resolution once the paint-time box size is known).
fn parse_linear_gradient_angle(first_seg: &str) -> (f32, Option<GradientCorner>) {
    let s = first_seg.trim().to_ascii_lowercase();

    // "to <side>" keywords.
    if s.starts_with("to ") {
        return match s.trim_start_matches("to ").trim() {
            "top" => (0.0, None),
            "right" => (90.0, None),
            "bottom" => (180.0, None),
            "left" => (270.0, None),
            "top right" | "right top" => (45.0, Some(GradientCorner::TopRight)),
            "bottom right" | "right bottom" => (135.0, Some(GradientCorner::BottomRight)),
            "bottom left" | "left bottom" => (225.0, Some(GradientCorner::BottomLeft)),
            "top left" | "left top" => (315.0, Some(GradientCorner::TopLeft)),
            _ => (180.0, None), // fallback: to bottom
        };
    }

    // Explicit angle unit.
    if let Some(deg) = s.strip_suffix("deg").and_then(|v| v.trim().parse::<f32>().ok()) {
        return (deg, None);
    }
    if let Some(turn) = s.strip_suffix("turn").and_then(|v| v.trim().parse::<f32>().ok()) {
        return (turn * 360.0, None);
    }
    if let Some(rad) = s.strip_suffix("rad").and_then(|v| v.trim().parse::<f32>().ok()) {
        return (rad * 180.0 / std::f32::consts::PI, None);
    }
    if let Some(grad) = s.strip_suffix("grad").and_then(|v| v.trim().parse::<f32>().ok()) {
        return (grad * 0.9, None); // 400 grad = 360 deg
    }

    // No recognised angle — default is "to bottom" per CSS spec.
    (180.0, None)
}

/// Parse `[circle|ellipse] [size] [at <x> <y>]` from the first segment of a
/// `radial-gradient`. Returns `(center_x, center_y)` as fractions [0, 1].
fn parse_radial_gradient_center(first_seg: &str) -> (f32, f32) {
    let s = first_seg.trim().to_ascii_lowercase();
    if let Some(at_idx) = s.find(" at ") {
        let pos = s[at_idx + 4..].trim();
        let parts: Vec<&str> = pos.split_whitespace().collect();
        let cx = parse_pct_or_keyword_x(parts.first().copied().unwrap_or("50%"));
        let cy = parse_pct_or_keyword_y(parts.get(1).copied().unwrap_or("50%"));
        return (cx, cy);
    }
    // Default centre = 50% 50%.
    (0.5, 0.5)
}

/// CSS Images L3 §3.5 — parse the ending-shape and size keywords from the first
/// segment of a `radial-gradient` (the part before any `at <position>`).
///
/// Recognises the `circle`/`ellipse` shape keyword and the four extent keywords
/// (`closest-side`, `closest-corner`, `farthest-side`, `farthest-corner`).
/// Defaults per spec: shape = `ellipse`, size = `farthest-corner`. A lone
/// `circle` keyword with no size still defaults to farthest-corner. Explicit
/// `<length>` radii are not modelled yet and leave the size at its default.
fn parse_radial_gradient_shape_size(first_seg: &str) -> (RadialShape, RadialSize) {
    // Only the prelude before `at` carries shape/size.
    let s = first_seg.trim().to_ascii_lowercase();
    let prelude = match s.find(" at ") {
        Some(i) => &s[..i],
        None if s.starts_with("at ") => "",
        None => s.as_str(),
    };
    let mut shape: Option<RadialShape> = None;
    let mut size: Option<RadialSize> = None;
    for tok in prelude.split_whitespace() {
        match tok {
            "circle" => shape = Some(RadialShape::Circle),
            "ellipse" => shape = Some(RadialShape::Ellipse),
            "closest-side" => size = Some(RadialSize::ClosestSide),
            "closest-corner" => size = Some(RadialSize::ClosestCorner),
            "farthest-side" => size = Some(RadialSize::FarthestSide),
            "farthest-corner" => size = Some(RadialSize::FarthestCorner),
            _ => {}
        }
    }
    (
        shape.unwrap_or(RadialShape::Ellipse),
        size.unwrap_or(RadialSize::FarthestCorner),
    )
}

/// Parse `[from <angle>]? [at <x> <y>]?` from the first segment of a
/// `conic-gradient`. Returns `(from_angle_deg, center_x, center_y)`.
/// Defaults: `from 0deg at 50% 50%`.
fn parse_conic_gradient_params(first_seg: &str) -> (f32, f32, f32) {
    let s = first_seg.trim().to_ascii_lowercase();
    // If the first segment starts with a color, treat as no positioning hint.
    if s.is_empty() || (!s.starts_with("from") && !s.starts_with("at")) {
        return (0.0, 0.5, 0.5);
    }

    // Extract `from <angle>` clause.
    let mut from_deg = 0.0_f32;
    let mut rest = s.clone();
    if let Some(stripped) = rest.strip_prefix("from") {
        let rest_trim = stripped.trim_start();
        // Find boundary: next whitespace after the angle token (may be
        // followed by `at ...`).
        let at_pos = rest_trim.find(" at ");
        let angle_tok = match at_pos {
            Some(idx) => rest_trim[..idx].trim(),
            None => rest_trim.trim(),
        };
        if let Some(rad) = parse_angle_to_radians(angle_tok) {
            from_deg = rad.to_degrees();
        }
        rest = match at_pos {
            Some(idx) => rest_trim[idx + 1..].to_string(), // includes "at ..."
            None => String::new(),
        };
    }

    // Extract `at <x> <y>` clause (rest may begin with "at ").
    let rest = rest.trim();
    let (cx, cy) = if let Some(pos) = rest.strip_prefix("at ") {
        let parts: Vec<&str> = pos.split_whitespace().collect();
        let cx = parse_pct_or_keyword_x(parts.first().copied().unwrap_or("50%"));
        let cy = parse_pct_or_keyword_y(parts.get(1).copied().unwrap_or("50%"));
        (cx, cy)
    } else {
        (0.5, 0.5)
    };

    (from_deg, cx, cy)
}

fn parse_pct_or_keyword_x(s: &str) -> f32 {
    match s {
        "left" => 0.0,
        "center" => 0.5,
        "right" => 1.0,
        _ => {
            if let Some(p) = s.strip_suffix('%').and_then(|v| v.parse::<f32>().ok()) {
                p / 100.0
            } else {
                0.5
            }
        }
    }
}

fn parse_pct_or_keyword_y(s: &str) -> f32 {
    match s {
        "top" => 0.0,
        "center" => 0.5,
        "bottom" => 1.0,
        _ => {
            if let Some(p) = s.strip_suffix('%').and_then(|v| v.parse::<f32>().ok()) {
                p / 100.0
            } else {
                0.5
            }
        }
    }
}

/// The leading direction / angle / shape argument (e.g. `to right`,
/// `45deg`, `circle at 50%`) is detected by the absence of a parseable
/// `<color>` and silently skipped. Color hints (bare `<length-percentage>`
/// without a color) are also skipped. Two-position stops (`red 20% 40%`)
/// expand to two `GradientStop`s per CSS Images L4 §3.4.
///
/// For `conic-gradient(...)` (and `repeating-conic-gradient(...)`), stop
/// positions accept `<angle>` units (`deg`/`rad`/`turn`/`grad`) in addition
/// to `<percentage>` per CSS Images L4 §3.7. Angles are normalised to
/// `Length::Percent` where 360° (1 full turn) maps to 100% — this lets the
/// downstream renderer treat conic and linear/radial stops uniformly.
#[allow(clippy::unwrap_used)]  // унаследовано, docs/lint-policy.md §10
pub fn parse_gradient_stops(s: &str) -> Vec<GradientStop> {
    let s = s.trim();
    let lower = s.to_ascii_lowercase();
    let is_conic =
        lower.starts_with("conic-gradient") || lower.starts_with("repeating-conic-gradient");

    let Some(open) = s.find('(') else {
        return vec![];
    };
    let rest = &s[open + 1..];
    let Some(close) = rest.rfind(')') else {
        return vec![];
    };
    let inner = rest[..close].trim();

    let segments = split_top_level_commas(inner);
    let mut stops: Vec<GradientStop> = Vec::new();

    let parse_pos = |t: &str| -> Option<Length> {
        if is_conic {
            parse_conic_stop_position(t)
        } else {
            parse_length_q(t, false)
        }
    };

    for seg in &segments {
        let tokens = paren_whitespace_tokens(seg.trim());
        if tokens.is_empty() {
            continue;
        }
        // Locate the first token that parses as a CSS color.
        // Segments without any color are direction/angle specifiers or
        // color hints — both are skipped.
        let Some(ci) = tokens.iter().position(|t| parse_color(t).is_some()) else {
            continue;
        };
        let color = parse_color(&tokens[ci]).unwrap();

        // CSS Images L3/L4: `<color> [ <length-percentage>{1,2} ]?`
        let pos1 = tokens.get(ci + 1).and_then(|t| parse_pos(t));
        let pos2 = tokens.get(ci + 2).and_then(|t| parse_pos(t));

        stops.push(GradientStop {
            color,
            position: pos1,
            ..Default::default()
        });
        if pos2.is_some() {
            stops.push(GradientStop {
                color,
                position: pos2,
                ..Default::default()
            });
        }
    }

    stops
}

/// Parse a conic-gradient stop position: `<angle>` or `<percentage>` per
/// CSS Images L4 §3.7. Angles (`deg`/`rad`/`turn`/`grad`) are converted to
/// `Length::Percent` with 360° → 100%, so downstream renderer logic can
/// treat all gradient kinds uniformly.
fn parse_conic_stop_position(s: &str) -> Option<Length> {
    let s = s.trim();
    // Percentage — pass through.
    if let Some(num) = s.strip_suffix('%')
        && let Ok(v) = num.trim().parse::<f32>()
    {
        return Some(Length::Percent(v));
    }
    // Angle units — convert to percent (360° = 100%).
    for (suffix, factor_deg) in [
        ("deg", 1.0_f32),
        ("turn", 360.0),
        ("grad", 0.9),
    ] {
        if let Some(num) = s.strip_suffix(suffix)
            && let Ok(v) = num.trim().parse::<f32>()
        {
            return Some(Length::Percent(v * factor_deg / 360.0 * 100.0));
        }
    }
    if let Some(num) = s.strip_suffix("rad")
        && let Ok(v) = num.trim().parse::<f32>()
    {
        return Some(Length::Percent(v.to_degrees() / 360.0 * 100.0));
    }
    // Unitless 0 — treat as 0°.
    if let Ok(0.0) = s.parse::<f32>() {
        return Some(Length::Percent(0.0));
    }
    None
}
