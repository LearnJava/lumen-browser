//! Парсинг `srcset` атрибута и выбор лучшего кандидата (HTML5
//! §4.8.4.3.5 «Parsing a srcset attribute» + §4.8.4.3.7 «Selecting an
//! image source»).
//!
//! Используется для `<img srcset>` и `<source srcset>` (внутри
//! `<picture>`). Реализация Phase 0:
//!   * lenient parser для типичных форм `url Nx, url Nw, url`;
//!   * descriptor parsing — `Nx` (density) и `Nw` (width);
//!   * picker по DPR для density-descriptors (`Nx`).
//!
//! W-descriptors (`Nw`) парсятся и сохраняются, но picker их сейчас
//! игнорирует — для них нужен `sizes` атрибут (CSS-like media query
//! list) и viewport width; это отдельная задача. До тех пор pick_best
//! работает только на x-descriptor-кандидатах.

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
}
