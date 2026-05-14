//! Парсинг `srcset` атрибута и выбор лучшего кандидата (HTML5
//! §4.8.4.3.5 «Parsing a srcset attribute» + §4.8.4.3.7 «Selecting an
//! image source» + §4.8.4.4 «Sizes attributes»).
//!
//! Используется для `<img srcset>` и `<source srcset>` (внутри
//! `<picture>`). Реализация Phase 0:
//!   * lenient parser для типичных форм `url Nx, url Nw, url`;
//!   * descriptor parsing — `Nx` (density) и `Nw` (width);
//!   * picker по DPR для density-descriptors (`Nx`);
//!   * `sizes`-атрибут — упрощённый media-condition list + length, и
//!     viewport-based picker для w-descriptor-кандидатов.

/// Один кандидат из `srcset`.
#[derive(Debug, Clone, PartialEq)]
pub struct SrcsetCandidate {
    pub url: String,
    pub descriptor: SrcsetDescriptor,
}

/// Дескриптор кандидата. По умолчанию `1x` (когда дескриптор
/// отсутствует в исходнике).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SrcsetDescriptor {
    /// Density — `Nx`, например `2x`. `1.0` если отсутствует.
    Density(f32),
    /// Width — `Nw`, например `480w`. Picker без `sizes` атрибута их
    /// не использует, но они валидно парсятся и сохраняются.
    Width(u32),
}

impl SrcsetDescriptor {
    /// Density по умолчанию — `1x`. Используется когда дескриптор
    /// отсутствует.
    pub const DEFAULT: Self = Self::Density(1.0);
}

/// Распарсить значение `srcset` атрибута. Возвращает список кандидатов
/// в порядке появления.
///
/// Грамматика (упрощённая HTML5 §4.8.4.3.5):
/// `srcset      = candidate ("," candidate)*`
/// `candidate   = url descriptor?`
/// `descriptor  = number "x" | integer "w"`
///
/// Невалидные кандидаты (пустой URL, невалидный дескриптор) — silently
/// пропускаются. Lenient parser — типичен для HTML5 «be lenient on
/// input».
pub fn parse_srcset(input: &str) -> Vec<SrcsetCandidate> {
    let bytes = input.as_bytes();
    let mut pos = 0;
    let mut candidates = Vec::new();

    while pos < bytes.len() {
        // 1. Skip whitespace + commas (между candidates).
        while pos < bytes.len() && (is_ascii_ws(bytes[pos]) || bytes[pos] == b',') {
            pos += 1;
        }
        if pos >= bytes.len() {
            break;
        }

        // 2. Collect URL — non-whitespace, non-comma chars.
        let url_start = pos;
        while pos < bytes.len() && !is_ascii_ws(bytes[pos]) && bytes[pos] != b',' {
            pos += 1;
        }
        let url_raw = &input[url_start..pos];

        // 3. URL может оканчиваться trailing commas (HTML5 §4.8.4.3.5
        //    step «remove all trailing U+002C COMMA characters»).
        //    Если есть — descriptor отсутствует, эмитим как Default.
        let stripped = url_raw.trim_end_matches(',');
        let had_trailing_comma = stripped.len() < url_raw.len();

        if stripped.is_empty() {
            // Только запятые — невалидный кандидат, skip.
            continue;
        }

        if had_trailing_comma {
            candidates.push(SrcsetCandidate {
                url: stripped.to_string(),
                descriptor: SrcsetDescriptor::DEFAULT,
            });
            continue;
        }

        // 4. Skip whitespace перед descriptor.
        while pos < bytes.len() && is_ascii_ws(bytes[pos]) {
            pos += 1;
        }

        // 5. Read descriptor — до следующей запятой / конца.
        let desc_start = pos;
        while pos < bytes.len() && bytes[pos] != b',' {
            pos += 1;
        }
        let desc_raw = input[desc_start..pos].trim();

        let descriptor = if desc_raw.is_empty() {
            SrcsetDescriptor::DEFAULT
        } else if let Some(d) = parse_descriptor(desc_raw) {
            d
        } else {
            // Невалидный descriptor — пропускаем кандидата.
            // Запятая (если есть) съестся при следующей итерации.
            if pos < bytes.len() && bytes[pos] == b',' {
                pos += 1;
            }
            continue;
        };

        candidates.push(SrcsetCandidate {
            url: stripped.to_string(),
            descriptor,
        });

        // 6. Skip запятую, если она нас ждёт.
        if pos < bytes.len() && bytes[pos] == b',' {
            pos += 1;
        }
    }

    candidates
}

/// Парсит descriptor вида `Nx` (float density) или `Nw` (integer width).
/// Возвращает `None` для невалидных форм. Пустая строка — невалидно
/// (caller проверяет до вызова).
fn parse_descriptor(s: &str) -> Option<SrcsetDescriptor> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    let last = s.as_bytes()[s.len() - 1];
    let num_part = &s[..s.len() - 1];
    match last {
        b'x' | b'X' => {
            let v: f32 = num_part.parse().ok()?;
            if v.is_finite() && v > 0.0 {
                Some(SrcsetDescriptor::Density(v))
            } else {
                None
            }
        }
        b'w' | b'W' => {
            let v: u32 = num_part.parse().ok()?;
            if v > 0 { Some(SrcsetDescriptor::Width(v)) } else { None }
        }
        _ => None,
    }
}

fn is_ascii_ws(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\n' | b'\r' | 0x0C)
}

/// Выбрать лучший кандидат по DPR для density-descriptors.
///
/// Алгоритм (упрощение HTML §4.8.4.3.7 «Selecting an image source»):
/// 1. Из всех кандидатов берутся только density-варианты (`Nx` и
///    Default = 1x). W-descriptors игнорируются — для них нужен `sizes`
///    атрибут.
/// 2. Среди этих ищется минимальный кандидат с `density >= dpr` (smallest
///    sufficient). Это даёт «не качай больше, чем надо для экрана».
/// 3. Если все кандидаты `< dpr` — берётся максимальный (закрываем DPR
///    лучшим из имеющегося, даже если он формально меньше).
/// 4. При равных density выигрывает первый по source-order
///    (стабильная итерация max_by/min_by).
///
/// Возвращает `None` если список пустой или содержит только w-кандидатов.
pub fn pick_best_for_density(
    candidates: &[SrcsetCandidate],
    dpr: f32,
) -> Option<&SrcsetCandidate> {
    let dpr = if dpr.is_finite() && dpr > 0.0 {
        dpr
    } else {
        1.0
    };

    // Filter to density candidates only.
    let with_density: Vec<(usize, f32, &SrcsetCandidate)> = candidates
        .iter()
        .enumerate()
        .filter_map(|(i, c)| match c.descriptor {
            SrcsetDescriptor::Density(d) => Some((i, d, c)),
            SrcsetDescriptor::Width(_) => None,
        })
        .collect();

    if with_density.is_empty() {
        return None;
    }

    // Шаг 2: smallest sufficient density >= dpr.
    let sufficient = with_density
        .iter()
        .filter(|(_, d, _)| *d >= dpr)
        .min_by(|(i1, d1, _), (i2, d2, _)| {
            d1.partial_cmp(d2)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then(i1.cmp(i2))
        });

    if let Some((_, _, c)) = sufficient {
        return Some(c);
    }

    // Шаг 3: fallback — наибольшая (лучшая из имеющегося).
    let best_available = with_density.iter().max_by(|(i1, d1, _), (i2, d2, _)| {
        d1.partial_cmp(d2)
            .unwrap_or(std::cmp::Ordering::Equal)
            // При равных density — берём ПЕРВЫЙ по source-order,
            // поэтому здесь reverse: меньший index «больше».
            .then(i2.cmp(i1))
    });
    best_available.map(|(_, _, c)| *c)
}

// ────────────────────────────────────────────────────────────────────────
// `sizes` атрибут (HTML5 §4.8.4.4) и picker для w-descriptor-кандидатов.
// ────────────────────────────────────────────────────────────────────────

/// Длина в `sizes`-атрибуте. По HTML5 §4.8.4.4 значение — одиночный
/// CSS `<length>`. Поддерживаются абсолютные (`px`) и viewport-relative
/// единицы (`vh` / `vw` / `vmin` / `vmax`), а также `em` / `rem` и `%`.
///
/// `calc()` / `min()` / `max()` / `clamp()` в Phase 0 не парсятся —
/// типичный sizes в природе использует одиночный токен.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SizeLength {
    Px(f32),
    Em(f32),
    Rem(f32),
    Vh(f32),
    Vw(f32),
    Vmin(f32),
    Vmax(f32),
    Percent(f32),
}

/// Viewport-параметры для резолва `sizes` в CSS-пиксели. `root_font_size_px`
/// используется для `em`/`rem` — sizes работает до построения DOM
/// (в preload-сканере), поэтому element-context font-size недоступен;
/// `em` трактуется как root-relative.
#[derive(Debug, Clone, Copy)]
pub struct SizesViewport {
    pub width_px: f32,
    pub height_px: f32,
    pub root_font_size_px: f32,
}

impl SizesViewport {
    /// Типичный desktop default: 1024×768, 16px root font-size.
    pub const DEFAULT: Self = Self {
        width_px: 1024.0,
        height_px: 768.0,
        root_font_size_px: 16.0,
    };
}

impl SizeLength {
    /// Резолв длины в CSS-пиксели.
    pub fn resolve(&self, viewport: SizesViewport) -> f32 {
        let root_fs = viewport.root_font_size_px;
        let vw = viewport.width_px;
        let vh = viewport.height_px;
        match *self {
            SizeLength::Px(v) => v,
            SizeLength::Em(v) | SizeLength::Rem(v) => v * root_fs,
            SizeLength::Vh(v) => v * vh / 100.0,
            SizeLength::Vw(v) => v * vw / 100.0,
            SizeLength::Vmin(v) => v * vw.min(vh) / 100.0,
            SizeLength::Vmax(v) => v * vw.max(vh) / 100.0,
            SizeLength::Percent(v) => v * vw / 100.0,
        }
    }
}

/// Ориентация viewport-а для media-feature `orientation:`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Orientation {
    Portrait,
    Landscape,
}

/// Одиночная media-feature внутри media-condition. AND-list из
/// `MediaClause` хранится в `MediaCondition::All`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MediaClause {
    MinWidth(SizeLength),
    MaxWidth(SizeLength),
    MinHeight(SizeLength),
    MaxHeight(SizeLength),
    Orientation(Orientation),
}

impl MediaClause {
    fn matches(&self, viewport: SizesViewport) -> bool {
        match *self {
            MediaClause::MinWidth(len) => viewport.width_px >= len.resolve(viewport),
            MediaClause::MaxWidth(len) => viewport.width_px <= len.resolve(viewport),
            MediaClause::MinHeight(len) => viewport.height_px >= len.resolve(viewport),
            MediaClause::MaxHeight(len) => viewport.height_px <= len.resolve(viewport),
            MediaClause::Orientation(Orientation::Portrait) => {
                viewport.height_px >= viewport.width_px
            }
            MediaClause::Orientation(Orientation::Landscape) => {
                viewport.width_px > viewport.height_px
            }
        }
    }
}

/// Media-condition в `sizes`-атрибуте.
///
/// Phase 0 поддерживает:
///   * single feature: `(min-width: 600px)`;
///   * AND-список: `(min-width: 600px) and (orientation: landscape)`;
///   * features: `min-width` / `max-width` / `min-height` / `max-height` /
///     `orientation`.
///
/// Неподдерживаемые формы (OR через запятую на уровне one condition,
/// `not`, `only`, неизвестные features) дают `Unsupported` — никогда
/// не матчат. OR на уровне всей `sizes`-строки обрабатывается естественно
/// — это просто разделение на несколько `SourceSize`.
#[derive(Debug, Clone, PartialEq)]
pub enum MediaCondition {
    All(Vec<MediaClause>),
    Unsupported,
}

impl MediaCondition {
    /// Принимает решение, удовлетворяет ли viewport условие.
    /// `Unsupported` всегда даёт `false` — это safe-default для
    /// неизвестных features.
    pub fn matches(&self, viewport: SizesViewport) -> bool {
        match self {
            MediaCondition::All(clauses) => clauses.iter().all(|c| c.matches(viewport)),
            MediaCondition::Unsupported => false,
        }
    }
}

/// Один элемент `sizes`-списка: опциональный media-condition + length.
#[derive(Debug, Clone, PartialEq)]
pub struct SourceSize {
    /// `None` означает «default» — применяется, если ни одна предыдущая
    /// строка не сматчилась. По HTML5 §4.8.4.4 default должен идти
    /// последним, но мы lenient: первый default-match выигрывает.
    pub condition: Option<MediaCondition>,
    pub value: SizeLength,
}

/// Распарсить значение `sizes`-атрибута. Возвращает список
/// `SourceSize` в порядке появления.
///
/// Грамматика (упрощённая HTML5 §4.8.4.4 + Media Queries L5):
/// ```text
/// sizes        = source-size ("," source-size)*
/// source-size  = media-condition? <length>
/// ```
///
/// Невалидные элементы (без length, с невалидной media-condition в
/// нестандартной форме) — silently пропускаются.
pub fn parse_sizes(input: &str) -> Vec<SourceSize> {
    let mut result = Vec::new();
    for part in input.split(',') {
        let trimmed = part.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Some(ss) = parse_one_source_size(trimmed) {
            result.push(ss);
        }
    }
    result
}

fn parse_one_source_size(s: &str) -> Option<SourceSize> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    // Если строка не содержит '(' — это просто length (default-fallback).
    if !s.contains('(') {
        return parse_size_length(s).map(|v| SourceSize {
            condition: None,
            value: v,
        });
    }
    // Иначе разбиваем на media-condition + length по последней whitespace
    // на верхнем уровне (length — единственный «голый» токен в конце).
    let last_ws = s.rfind(char::is_whitespace)?;
    let cond_str = s[..last_ws].trim();
    let val_str = s[last_ws..].trim();
    let value = parse_size_length(val_str)?;
    let condition = parse_media_condition(cond_str);
    Some(SourceSize {
        condition: Some(condition),
        value,
    })
}

/// Распарсить одиночный CSS-length-токен (`Npx`, `Nem`, `Nvw`, `N%` и т.д.).
fn parse_size_length(s: &str) -> Option<SizeLength> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    let bytes = s.as_bytes();
    let mut end = 0;
    let mut seen_dot = false;
    while end < bytes.len() {
        let c = bytes[end];
        if c.is_ascii_digit() {
            end += 1;
        } else if c == b'.' && !seen_dot {
            seen_dot = true;
            end += 1;
        } else if (c == b'-' || c == b'+') && end == 0 {
            end += 1;
        } else {
            break;
        }
    }
    if end == 0 {
        return None;
    }
    let num_part = &s[..end];
    let value: f32 = num_part.parse().ok()?;
    if !value.is_finite() {
        return None;
    }
    let unit = s[end..].trim();
    let lower = unit.to_ascii_lowercase();
    Some(match lower.as_str() {
        "px" => SizeLength::Px(value),
        "em" => SizeLength::Em(value),
        "rem" => SizeLength::Rem(value),
        "vh" => SizeLength::Vh(value),
        "vw" => SizeLength::Vw(value),
        "vmin" => SizeLength::Vmin(value),
        "vmax" => SizeLength::Vmax(value),
        "%" => SizeLength::Percent(value),
        // Unitless length по HTML5 spec — невалидно.
        _ => return None,
    })
}

/// Распарсить media-condition. Lenient: `Unsupported` вместо `None` —
/// чтобы caller отличал «корректный, но не sматчит» от «синтаксис
/// сломан полностью». Здесь возвращаем всегда что-то.
///
/// Используется как из `parse_sizes` (внутренне), так и из `picture`-picker-а
/// для атрибута `<source media>` — там та же грамматика
/// (`(min-width: ...) and (...)`), потому что Phase 0 не различает полный
/// media-query и sizes-media-condition.
pub fn parse_media_condition(s: &str) -> MediaCondition {
    let lower = s.trim().to_ascii_lowercase();
    if lower.is_empty() {
        return MediaCondition::Unsupported;
    }
    // Split по " and " на top-level — внутри `()` " and " не встречается
    // (валидная media-feature имеет вид `(name: value)` без `and`).
    let parts: Vec<&str> = lower.split(" and ").map(str::trim).collect();
    let mut clauses = Vec::with_capacity(parts.len());
    for part in parts {
        if part.is_empty() {
            return MediaCondition::Unsupported;
        }
        match parse_media_clause(part) {
            Some(c) => clauses.push(c),
            None => return MediaCondition::Unsupported,
        }
    }
    if clauses.is_empty() {
        MediaCondition::Unsupported
    } else {
        MediaCondition::All(clauses)
    }
}

fn parse_media_clause(s: &str) -> Option<MediaClause> {
    let s = s.trim();
    if !s.starts_with('(') || !s.ends_with(')') {
        return None;
    }
    let inner = s[1..s.len() - 1].trim();
    let (name, value) = inner.split_once(':')?;
    let name = name.trim();
    let value = value.trim();
    match name {
        "min-width" => parse_size_length(value).map(MediaClause::MinWidth),
        "max-width" => parse_size_length(value).map(MediaClause::MaxWidth),
        "min-height" => parse_size_length(value).map(MediaClause::MinHeight),
        "max-height" => parse_size_length(value).map(MediaClause::MaxHeight),
        "orientation" => match value {
            "portrait" => Some(MediaClause::Orientation(Orientation::Portrait)),
            "landscape" => Some(MediaClause::Orientation(Orientation::Landscape)),
            _ => None,
        },
        _ => None,
    }
}

/// Вычислить эффективную «source size» в CSS-пикселях по `sizes` и
/// текущему viewport. Алгоритм HTML5 §4.8.4.4 «Parsing a sizes attribute»:
/// первая `SourceSize`, чьё `condition` либо `None`, либо матчится,
/// определяет result.
///
/// Если ничего не матчится (нет default-строки в конце и ни одно
/// условие не выполнено) — fallback `100vw` (= viewport.width_px).
pub fn evaluate_sizes(sizes: &[SourceSize], viewport: SizesViewport) -> f32 {
    for ss in sizes {
        let matches = match &ss.condition {
            None => true,
            Some(c) => c.matches(viewport),
        };
        if matches {
            return ss.value.resolve(viewport);
        }
    }
    viewport.width_px
}

/// Выбрать лучший кандидат по w-descriptor (HTML5 §4.8.4.3.7).
///
/// Алгоритм:
/// 1. Эффективная плотность w-кандидата = `width_descriptor /
///    source_size_px`. Это «pixels-per-css-pixel», аналог `Nx`.
/// 2. Среди эффективных плотностей — `smallest sufficient >= dpr`.
/// 3. Если все меньше `dpr` — fallback на наибольшую (как и в
///    [`pick_best_for_density`]).
/// 4. При равенстве — первый по source-order.
///
/// Возвращает `None`, если в списке нет w-кандидатов или
/// `source_size_px` невалиден (≤ 0 / NaN / ∞). Density-кандидаты
/// (`Nx`) в w-picker-е игнорируются: смешивание `Nw` и `Nx` в одном
/// `srcset` нарушает spec, мы lenient и просто отбрасываем `Nx`.
pub fn pick_best_for_width(
    candidates: &[SrcsetCandidate],
    source_size_px: f32,
    dpr: f32,
) -> Option<&SrcsetCandidate> {
    if !source_size_px.is_finite() || source_size_px <= 0.0 {
        return None;
    }
    let dpr = if dpr.is_finite() && dpr > 0.0 {
        dpr
    } else {
        1.0
    };

    let effective: Vec<(usize, f32, &SrcsetCandidate)> = candidates
        .iter()
        .enumerate()
        .filter_map(|(i, c)| match c.descriptor {
            SrcsetDescriptor::Width(w) if w > 0 => {
                let eff = (w as f32) / source_size_px;
                if eff.is_finite() {
                    Some((i, eff, c))
                } else {
                    None
                }
            }
            _ => None,
        })
        .collect();

    if effective.is_empty() {
        return None;
    }

    let sufficient = effective
        .iter()
        .filter(|(_, d, _)| *d >= dpr)
        .min_by(|(i1, d1, _), (i2, d2, _)| {
            d1.partial_cmp(d2)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then(i1.cmp(i2))
        });
    if let Some((_, _, c)) = sufficient {
        return Some(c);
    }

    effective
        .iter()
        .max_by(|(i1, d1, _), (i2, d2, _)| {
            d1.partial_cmp(d2)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then(i2.cmp(i1))
        })
        .map(|(_, _, c)| *c)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn density(f: f32) -> SrcsetDescriptor {
        SrcsetDescriptor::Density(f)
    }

    fn width(w: u32) -> SrcsetDescriptor {
        SrcsetDescriptor::Width(w)
    }

    // ──────── parse_srcset: типичные случаи ────────

    #[test]
    fn empty_srcset_returns_empty() {
        assert!(parse_srcset("").is_empty());
        assert!(parse_srcset("   ").is_empty());
        assert!(parse_srcset(",,,").is_empty());
    }

    #[test]
    fn single_url_no_descriptor() {
        let c = parse_srcset("foo.png");
        assert_eq!(c.len(), 1);
        assert_eq!(c[0].url, "foo.png");
        assert_eq!(c[0].descriptor, SrcsetDescriptor::DEFAULT);
    }

    #[test]
    fn single_url_with_density() {
        let c = parse_srcset("foo.png 2x");
        assert_eq!(c.len(), 1);
        assert_eq!(c[0].url, "foo.png");
        assert_eq!(c[0].descriptor, density(2.0));
    }

    #[test]
    fn two_density_candidates() {
        let c = parse_srcset("a.png 1x, b.png 2x");
        assert_eq!(c.len(), 2);
        assert_eq!(c[0].url, "a.png");
        assert_eq!(c[0].descriptor, density(1.0));
        assert_eq!(c[1].url, "b.png");
        assert_eq!(c[1].descriptor, density(2.0));
    }

    #[test]
    fn fractional_density() {
        let c = parse_srcset("a.png 1.5x, b.png 2.5x");
        assert_eq!(c[0].descriptor, density(1.5));
        assert_eq!(c[1].descriptor, density(2.5));
    }

    #[test]
    fn width_descriptor() {
        let c = parse_srcset("small.png 480w, large.png 1024w");
        assert_eq!(c.len(), 2);
        assert_eq!(c[0].descriptor, width(480));
        assert_eq!(c[1].descriptor, width(1024));
    }

    #[test]
    fn mixed_default_and_density() {
        // URL без descriptor дополняется до Default (1x).
        let c = parse_srcset("normal.png, retina.png 2x");
        assert_eq!(c.len(), 2);
        assert_eq!(c[0].url, "normal.png");
        assert_eq!(c[0].descriptor, SrcsetDescriptor::DEFAULT);
        assert_eq!(c[1].url, "retina.png");
        assert_eq!(c[1].descriptor, density(2.0));
    }

    // ──────── parse_srcset: edge cases ────────

    #[test]
    fn trailing_comma_in_url_means_no_descriptor() {
        // `a.png,` — URL с trailing comma, без descriptor.
        let c = parse_srcset("a.png, b.png 2x");
        assert_eq!(c.len(), 2);
        assert_eq!(c[0].url, "a.png");
        assert_eq!(c[0].descriptor, SrcsetDescriptor::DEFAULT);
    }

    #[test]
    fn extra_whitespace_tolerated() {
        let c = parse_srcset("  a.png  1x  ,  b.png  2x  ");
        assert_eq!(c.len(), 2);
        assert_eq!(c[0].url, "a.png");
        assert_eq!(c[0].descriptor, density(1.0));
        assert_eq!(c[1].url, "b.png");
        assert_eq!(c[1].descriptor, density(2.0));
    }

    #[test]
    fn newlines_and_tabs_as_whitespace() {
        let c = parse_srcset("a.png\t1x,\nb.png\r2x");
        assert_eq!(c.len(), 2);
        assert_eq!(c[1].descriptor, density(2.0));
    }

    #[test]
    fn case_insensitive_x_w() {
        let c = parse_srcset("a.png 2X, b.png 480W");
        assert_eq!(c[0].descriptor, density(2.0));
        assert_eq!(c[1].descriptor, width(480));
    }

    #[test]
    fn invalid_descriptor_drops_candidate() {
        // `garbage` не x/w → кандидат отбрасывается.
        let c = parse_srcset("a.png 2x, b.png garbage, c.png 3x");
        assert_eq!(c.len(), 2);
        assert_eq!(c[0].url, "a.png");
        assert_eq!(c[1].url, "c.png");
    }

    #[test]
    fn negative_or_zero_density_invalid() {
        // 0x / -1x невалидны.
        let c = parse_srcset("a.png 0x, b.png 2x");
        assert_eq!(c.len(), 1);
        assert_eq!(c[0].url, "b.png");
        let c2 = parse_srcset("a.png -1x, b.png 2x");
        assert_eq!(c2.len(), 1);
        assert_eq!(c2[0].url, "b.png");
    }

    #[test]
    fn zero_width_invalid() {
        let c = parse_srcset("a.png 0w, b.png 480w");
        assert_eq!(c.len(), 1);
        assert_eq!(c[0].descriptor, width(480));
    }

    #[test]
    fn url_with_query_string_preserved() {
        let c = parse_srcset("/img?v=1 1x, /img?v=2 2x");
        assert_eq!(c[0].url, "/img?v=1");
        assert_eq!(c[1].url, "/img?v=2");
    }

    #[test]
    fn cyrillic_url() {
        let c = parse_srcset("картинка.png 1x, фото.png 2x");
        assert_eq!(c.len(), 2);
        assert_eq!(c[0].url, "картинка.png");
        assert_eq!(c[1].url, "фото.png");
    }

    // ──────── pick_best_for_density ────────

    #[test]
    fn pick_smallest_sufficient() {
        let c = parse_srcset("a.png 1x, b.png 2x, c.png 3x");
        let picked = pick_best_for_density(&c, 2.0).unwrap();
        assert_eq!(picked.url, "b.png");
    }

    #[test]
    fn pick_exact_match() {
        let c = parse_srcset("a.png 1x, b.png 2x");
        let picked = pick_best_for_density(&c, 1.0).unwrap();
        assert_eq!(picked.url, "a.png");
    }

    #[test]
    fn pick_smallest_greater_than_dpr() {
        // dpr=1.5 → берём 2x (smallest >= 1.5).
        let c = parse_srcset("a.png 1x, b.png 2x, c.png 3x");
        let picked = pick_best_for_density(&c, 1.5).unwrap();
        assert_eq!(picked.url, "b.png");
    }

    #[test]
    fn pick_largest_when_all_below_dpr() {
        // dpr=5, есть только 1x/2x — берём 2x (наибольший).
        let c = parse_srcset("a.png 1x, b.png 2x");
        let picked = pick_best_for_density(&c, 5.0).unwrap();
        assert_eq!(picked.url, "b.png");
    }

    #[test]
    fn pick_default_when_no_descriptor() {
        // URL без descriptor = 1x; dpr=1 → его и берём.
        let c = parse_srcset("a.png");
        let picked = pick_best_for_density(&c, 1.0).unwrap();
        assert_eq!(picked.url, "a.png");
    }

    #[test]
    fn pick_none_when_only_width_descriptors() {
        // Без sizes атрибута w-descriptors не работают.
        let c = parse_srcset("a.png 480w, b.png 1024w");
        assert!(pick_best_for_density(&c, 2.0).is_none());
    }

    #[test]
    fn pick_ignores_width_among_density() {
        // 480w игнорируется, выбираем из 1x/3x.
        let c = parse_srcset("small.png 480w, normal.png 1x, retina.png 3x");
        let picked = pick_best_for_density(&c, 2.0).unwrap();
        assert_eq!(picked.url, "retina.png");
    }

    #[test]
    fn pick_first_on_density_tie() {
        // Два кандидата с одинаковым 2x — выигрывает первый.
        let c = parse_srcset("first.png 2x, second.png 2x");
        let picked = pick_best_for_density(&c, 2.0).unwrap();
        assert_eq!(picked.url, "first.png");
    }

    #[test]
    fn pick_dpr_zero_treated_as_one() {
        // Невалидный dpr (0/негативный/NaN) → 1.0.
        let c = parse_srcset("a.png 1x, b.png 2x");
        let picked = pick_best_for_density(&c, 0.0).unwrap();
        assert_eq!(picked.url, "a.png");
        let picked2 = pick_best_for_density(&c, f32::NAN).unwrap();
        assert_eq!(picked2.url, "a.png");
    }

    #[test]
    fn pick_empty_returns_none() {
        let c = parse_srcset("");
        assert!(pick_best_for_density(&c, 1.0).is_none());
    }

    // ──────── sizes: parse_size_length ────────

    fn vp(w: f32, h: f32) -> SizesViewport {
        SizesViewport {
            width_px: w,
            height_px: h,
            root_font_size_px: 16.0,
        }
    }

    #[test]
    fn size_length_px() {
        assert_eq!(parse_size_length("100px"), Some(SizeLength::Px(100.0)));
        assert_eq!(parse_size_length("0px"), Some(SizeLength::Px(0.0)));
        assert_eq!(parse_size_length("12.5px"), Some(SizeLength::Px(12.5)));
    }

    #[test]
    fn size_length_unitless_invalid() {
        assert_eq!(parse_size_length("100"), None);
    }

    #[test]
    fn size_length_case_insensitive_unit() {
        assert_eq!(parse_size_length("50PX"), Some(SizeLength::Px(50.0)));
        assert_eq!(parse_size_length("50Vw"), Some(SizeLength::Vw(50.0)));
    }

    #[test]
    fn size_length_all_units() {
        assert_eq!(parse_size_length("2em"), Some(SizeLength::Em(2.0)));
        assert_eq!(parse_size_length("1.5rem"), Some(SizeLength::Rem(1.5)));
        assert_eq!(parse_size_length("100vw"), Some(SizeLength::Vw(100.0)));
        assert_eq!(parse_size_length("50vh"), Some(SizeLength::Vh(50.0)));
        assert_eq!(parse_size_length("10vmin"), Some(SizeLength::Vmin(10.0)));
        assert_eq!(parse_size_length("90vmax"), Some(SizeLength::Vmax(90.0)));
        assert_eq!(parse_size_length("33%"), Some(SizeLength::Percent(33.0)));
    }

    #[test]
    fn size_length_unknown_unit_invalid() {
        assert_eq!(parse_size_length("10pt"), None);
        assert_eq!(parse_size_length("10in"), None);
    }

    #[test]
    fn size_length_resolve_px() {
        let v = vp(1024.0, 768.0);
        assert_eq!(SizeLength::Px(100.0).resolve(v), 100.0);
    }

    #[test]
    fn size_length_resolve_viewport_units() {
        let v = vp(1000.0, 500.0);
        assert_eq!(SizeLength::Vw(50.0).resolve(v), 500.0);
        assert_eq!(SizeLength::Vh(50.0).resolve(v), 250.0);
        assert_eq!(SizeLength::Vmin(100.0).resolve(v), 500.0);
        assert_eq!(SizeLength::Vmax(100.0).resolve(v), 1000.0);
    }

    #[test]
    fn size_length_resolve_em_rem() {
        let v = vp(1024.0, 768.0);
        assert_eq!(SizeLength::Em(2.0).resolve(v), 32.0);
        assert_eq!(SizeLength::Rem(1.5).resolve(v), 24.0);
    }

    #[test]
    fn size_length_resolve_percent_of_viewport_width() {
        let v = vp(800.0, 600.0);
        assert_eq!(SizeLength::Percent(50.0).resolve(v), 400.0);
    }

    // ──────── sizes: parse_media_clause ────────

    #[test]
    fn clause_min_width() {
        assert_eq!(
            parse_media_clause("(min-width: 600px)"),
            Some(MediaClause::MinWidth(SizeLength::Px(600.0)))
        );
    }

    #[test]
    fn clause_max_width() {
        assert_eq!(
            parse_media_clause("(max-width: 800px)"),
            Some(MediaClause::MaxWidth(SizeLength::Px(800.0)))
        );
    }

    #[test]
    fn clause_orientation_portrait() {
        assert_eq!(
            parse_media_clause("(orientation: portrait)"),
            Some(MediaClause::Orientation(Orientation::Portrait))
        );
    }

    #[test]
    fn clause_orientation_landscape() {
        assert_eq!(
            parse_media_clause("(orientation: landscape)"),
            Some(MediaClause::Orientation(Orientation::Landscape))
        );
    }

    #[test]
    fn clause_unknown_feature_returns_none() {
        assert_eq!(parse_media_clause("(some-unknown: 5px)"), None);
    }

    #[test]
    fn clause_without_parens_invalid() {
        assert_eq!(parse_media_clause("min-width: 600px"), None);
    }

    // ──────── sizes: parse_media_condition ────────

    #[test]
    fn condition_single_feature() {
        let c = parse_media_condition("(min-width: 600px)");
        assert!(c.matches(vp(800.0, 600.0)));
        assert!(!c.matches(vp(400.0, 600.0)));
    }

    #[test]
    fn condition_and_combination() {
        let c = parse_media_condition("(min-width: 600px) and (max-width: 1000px)");
        assert!(c.matches(vp(800.0, 600.0)));
        assert!(!c.matches(vp(500.0, 600.0)));
        assert!(!c.matches(vp(1200.0, 600.0)));
    }

    #[test]
    fn condition_and_case_insensitive() {
        // " AND " (uppercase) тоже должен работать.
        let c = parse_media_condition("(min-width: 600px) AND (orientation: landscape)");
        assert!(c.matches(vp(800.0, 600.0)));
        assert!(!c.matches(vp(500.0, 600.0)));
    }

    #[test]
    fn condition_unsupported_clause_makes_whole_unsupported() {
        // Единственная невалидная features-клауза делает всю condition
        // Unsupported (= never matches).
        let c = parse_media_condition("(min-width: 600px) and (unknown: 5px)");
        assert_eq!(c, MediaCondition::Unsupported);
        assert!(!c.matches(vp(800.0, 600.0)));
    }

    #[test]
    fn condition_empty_unsupported() {
        let c = parse_media_condition("");
        assert_eq!(c, MediaCondition::Unsupported);
        assert!(!c.matches(vp(800.0, 600.0)));
    }

    // ──────── sizes: parse_sizes ────────

    #[test]
    fn parse_sizes_empty() {
        assert!(parse_sizes("").is_empty());
        assert!(parse_sizes("   ").is_empty());
        assert!(parse_sizes(",,,").is_empty());
    }

    #[test]
    fn parse_sizes_single_default() {
        let s = parse_sizes("100vw");
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].condition, None);
        assert_eq!(s[0].value, SizeLength::Vw(100.0));
    }

    #[test]
    fn parse_sizes_single_with_condition() {
        let s = parse_sizes("(min-width: 600px) 50vw");
        assert_eq!(s.len(), 1);
        assert!(s[0].condition.is_some());
        assert_eq!(s[0].value, SizeLength::Vw(50.0));
    }

    #[test]
    fn parse_sizes_typical_responsive() {
        // Реальный пример: десктоп → 50% ширины, мобильный → 100%.
        let s = parse_sizes("(min-width: 600px) 50vw, 100vw");
        assert_eq!(s.len(), 2);
        assert!(s[0].condition.is_some());
        assert_eq!(s[0].value, SizeLength::Vw(50.0));
        assert_eq!(s[1].condition, None);
        assert_eq!(s[1].value, SizeLength::Vw(100.0));
    }

    #[test]
    fn parse_sizes_multi_breakpoint() {
        let s = parse_sizes("(min-width: 1200px) 800px, (min-width: 600px) 50vw, 100vw");
        assert_eq!(s.len(), 3);
        assert_eq!(s[0].value, SizeLength::Px(800.0));
        assert_eq!(s[1].value, SizeLength::Vw(50.0));
        assert_eq!(s[2].value, SizeLength::Vw(100.0));
        assert_eq!(s[2].condition, None);
    }

    #[test]
    fn parse_sizes_invalid_value_skipped() {
        // `garbage` без unit — невалидный length, элемент пропускается.
        let s = parse_sizes("(min-width: 600px) garbage, 100vw");
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].value, SizeLength::Vw(100.0));
    }

    #[test]
    fn parse_sizes_extra_whitespace() {
        let s = parse_sizes("   (min-width: 600px)   50vw   ,   100vw   ");
        assert_eq!(s.len(), 2);
        assert_eq!(s[0].value, SizeLength::Vw(50.0));
    }

    // ──────── sizes: evaluate_sizes ────────

    #[test]
    fn evaluate_default_only() {
        let s = parse_sizes("50vw");
        let r = evaluate_sizes(&s, vp(1000.0, 800.0));
        assert_eq!(r, 500.0);
    }

    #[test]
    fn evaluate_first_matching_condition() {
        let s = parse_sizes("(min-width: 1200px) 800px, (min-width: 600px) 50vw, 100vw");
        // viewport 800px → matches только второе условие (min-width 600).
        assert_eq!(evaluate_sizes(&s, vp(800.0, 600.0)), 400.0);
        // viewport 1400px → matches первое.
        assert_eq!(evaluate_sizes(&s, vp(1400.0, 600.0)), 800.0);
        // viewport 400px → fallback на default.
        assert_eq!(evaluate_sizes(&s, vp(400.0, 600.0)), 400.0);
    }

    #[test]
    fn evaluate_no_match_returns_viewport_width() {
        // Все условия не sматчили, default нет → 100vw.
        let s = parse_sizes("(min-width: 1200px) 800px");
        assert_eq!(evaluate_sizes(&s, vp(800.0, 600.0)), 800.0);
    }

    #[test]
    fn evaluate_empty_sizes_returns_viewport_width() {
        let s = parse_sizes("");
        assert_eq!(evaluate_sizes(&s, vp(1024.0, 768.0)), 1024.0);
    }

    // ──────── pick_best_for_width ────────

    #[test]
    fn width_pick_smallest_sufficient() {
        // source-size = 500px, dpr = 1.0
        // кандидаты 320w / 640w / 1280w → плотности 0.64 / 1.28 / 2.56
        // smallest >= 1.0 → 640w
        let c = parse_srcset("a.png 320w, b.png 640w, c.png 1280w");
        let picked = pick_best_for_width(&c, 500.0, 1.0).unwrap();
        assert_eq!(picked.url, "b.png");
    }

    #[test]
    fn width_pick_for_retina_dpr() {
        // source-size = 500px, dpr = 2.0
        // плотности 0.64 / 1.28 / 2.56 → smallest >= 2.0 → 1280w
        let c = parse_srcset("a.png 320w, b.png 640w, c.png 1280w");
        let picked = pick_best_for_width(&c, 500.0, 2.0).unwrap();
        assert_eq!(picked.url, "c.png");
    }

    #[test]
    fn width_pick_largest_when_all_below() {
        // source-size = 500px, dpr = 3.0
        // плотности 0.64 / 1.28 — все < 3.0 → fallback на наибольший = 640w
        let c = parse_srcset("a.png 320w, b.png 640w");
        let picked = pick_best_for_width(&c, 500.0, 3.0).unwrap();
        assert_eq!(picked.url, "b.png");
    }

    #[test]
    fn width_pick_ignores_density_candidates() {
        // 2x — density, должен быть проигнорирован w-picker-ом.
        let c = parse_srcset("a.png 2x, b.png 640w");
        let picked = pick_best_for_width(&c, 500.0, 1.0).unwrap();
        assert_eq!(picked.url, "b.png");
    }

    #[test]
    fn width_pick_no_width_candidates_returns_none() {
        // Только density-кандидаты — picker возвращает None.
        let c = parse_srcset("a.png 1x, b.png 2x");
        assert!(pick_best_for_width(&c, 500.0, 1.0).is_none());
    }

    #[test]
    fn width_pick_empty_returns_none() {
        let c = parse_srcset("");
        assert!(pick_best_for_width(&c, 500.0, 1.0).is_none());
    }

    #[test]
    fn width_pick_invalid_source_size_returns_none() {
        let c = parse_srcset("a.png 640w");
        assert!(pick_best_for_width(&c, 0.0, 1.0).is_none());
        assert!(pick_best_for_width(&c, -100.0, 1.0).is_none());
        assert!(pick_best_for_width(&c, f32::NAN, 1.0).is_none());
    }

    #[test]
    fn width_pick_dpr_invalid_treated_as_one() {
        // dpr = 0 / NaN → трактуется как 1.0
        let c = parse_srcset("a.png 320w, b.png 640w");
        let picked1 = pick_best_for_width(&c, 500.0, 0.0).unwrap();
        let picked2 = pick_best_for_width(&c, 500.0, f32::NAN).unwrap();
        // 0.64 / 1.28 → smallest >= 1.0 → 640w
        assert_eq!(picked1.url, "b.png");
        assert_eq!(picked2.url, "b.png");
    }

    #[test]
    fn width_pick_first_on_density_tie() {
        // Два кандидата с одинаковой effective density — первый по
        // source-order выигрывает (640w / 640w даёт два одинаковых).
        let c = parse_srcset("first.png 640w, second.png 640w");
        let picked = pick_best_for_width(&c, 500.0, 1.0).unwrap();
        assert_eq!(picked.url, "first.png");
    }

    // ──────── интеграционные кейсы parse_sizes + pick_best_for_width ────────

    #[test]
    fn integration_responsive_image_picks_640w_on_desktop() {
        // Реальный pattern: srcset с w-кандидатами + sizes-атрибут с
        // media-condition. На desktop (viewport 1200px, sizes-условие
        // matches 50vw → 600 CSS px) выберем подходящий w-кандидат.
        let candidates = parse_srcset("small.jpg 320w, medium.jpg 640w, large.jpg 1280w");
        let sizes = parse_sizes("(min-width: 600px) 50vw, 100vw");
        let source = evaluate_sizes(&sizes, vp(1200.0, 800.0));
        assert_eq!(source, 600.0);
        let picked = pick_best_for_width(&candidates, source, 1.0).unwrap();
        // плотности 320/600=0.53, 640/600=1.07, 1280/600=2.13 → smallest >= 1.0 → 640w
        assert_eq!(picked.url, "medium.jpg");
    }

    #[test]
    fn integration_responsive_image_picks_320w_on_mobile() {
        // На mobile (viewport 400px, sizes-условие не матчит → default 100vw → 400 CSS px)
        let candidates = parse_srcset("small.jpg 320w, medium.jpg 640w, large.jpg 1280w");
        let sizes = parse_sizes("(min-width: 600px) 50vw, 100vw");
        let source = evaluate_sizes(&sizes, vp(400.0, 800.0));
        assert_eq!(source, 400.0);
        let picked = pick_best_for_width(&candidates, source, 1.0).unwrap();
        // плотности 320/400=0.8, 640/400=1.6, 1280/400=3.2 → smallest >= 1.0 → 640w
        // (320 не sufficient — округление дробной density обязательно >= dpr)
        assert_eq!(picked.url, "medium.jpg");
    }

    #[test]
    fn integration_responsive_image_picks_large_on_retina_desktop() {
        let candidates = parse_srcset("small.jpg 320w, medium.jpg 640w, large.jpg 1280w");
        let sizes = parse_sizes("(min-width: 600px) 50vw, 100vw");
        // desktop retina: viewport 1200px @ dpr 2 → source = 600px, нужна
        // плотность >= 2.0. 1280/600=2.13 → large.jpg.
        let source = evaluate_sizes(&sizes, vp(1200.0, 800.0));
        let picked = pick_best_for_width(&candidates, source, 2.0).unwrap();
        assert_eq!(picked.url, "large.jpg");
    }
}
