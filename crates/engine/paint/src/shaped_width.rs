//! Ширина shaped run-а для раскладки (TEXTQ-5).
//!
//! Раскладка раньше мерила текст посимвольно (`advance` из `hmtx` по `cmap`),
//! а отрисовка с TEXTQ-4 шейпит: кернинг и лигатуры сдвигали нарисованный run
//! относительно места, которое ему отвёл бокс. Здесь измеритель считает ту же
//! сумму advance-ов тем же шейпером ([`lumen_font::active_text_shaper`]) с теми
//! же фичами и осями, что и `DrawText`.
//!
//! Цена шейпинга слова — микросекунды, а раскладка меряет одни и те же слова
//! снова и снова (каждый relayout, каждая проба ширины), поэтому результат
//! кэшируется по `(face, text, features, axes)` в единицах шрифта — без размера
//! и `letter-spacing`, чтобы один и тот же кэш работал для любого размера.

use std::cell::RefCell;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};

use lumen_layout::ShapeParams;

/// Шрифты крупнее не хранятся для шейпинга: байты живут столько же, сколько
/// метрики, а CJK-семейства на десятки мегабайт не нуждаются в кернинге
/// (там меряется посимвольно, как и раньше).
pub(crate) const MAX_SHAPEABLE_FONT_BYTES: usize = 8 << 20;

/// Сколько записей держит кэш на поток; при переполнении сбрасывается целиком
/// (слов на странице — тысячи, а не сотни тысяч).
const CACHE_MAX_ENTRIES: usize = 1 << 15;

/// Фичи шейпинга run-а (TEXTQ-4): `font-feature-settings` как есть, а при
/// ненулевом `letter-spacing` перед ними — выключенные `liga`/`clig`. CSS Text
/// L3 §8.2: UA не должен применять необязательные лигатуры, когда между
/// символами добавлен интервал. Выключатели идут ПЕРВЫМИ, поэтому явное
/// `"liga" 1` автора, стоящее позже, их перекрывает (так делают и Chromium, и
/// HarfBuzz — при совпадении диапазона побеждает последняя фича).
///
/// Единый источник для отрисовки (wgpu, CPU) и измерителя раскладки: иначе
/// нарисованная и отведённая под run ширина расходятся при `letter-spacing`.
pub(crate) fn shaping_features(
    font_features: &[([u8; 4], u32)],
    letter_spacing: f32,
) -> std::borrow::Cow<'_, [([u8; 4], u32)]> {
    if letter_spacing == 0.0 {
        return std::borrow::Cow::Borrowed(font_features);
    }
    let mut all = Vec::with_capacity(font_features.len() + 2);
    all.push((*b"liga", 0));
    all.push((*b"clig", 0));
    all.extend_from_slice(font_features);
    std::borrow::Cow::Owned(all)
}

/// `true`, если шейпинг отключён (`LUMEN_NO_TEXT_SHAPING=1`) — рычаг отката
/// TEXTQ-4/5 к укладке «глиф на кодпойнт из `cmap`, advance из `hmtx`», без
/// кернинга и `font-feature-settings`. Один рычаг на отрисовку и на измеритель
/// раскладки: иначе ширина бокса и нарисованного run-а расходятся.
pub(crate) fn text_shaping_disabled() -> bool {
    use std::sync::OnceLock;
    static OFF: OnceLock<bool> = OnceLock::new();
    *OFF.get_or_init(|| std::env::var("LUMEN_NO_TEXT_SHAPING").is_ok_and(|v| v == "1"))
}

/// Шрифт, которым меряется под-run.
#[derive(Clone, Copy)]
pub(crate) struct ShapeFace<'a> {
    pub(crate) bytes: &'a [u8],
    /// [`lumen_font::font_fingerprint`] от `bytes`.
    pub(crate) fingerprint: u64,
    pub(crate) units_per_em: u16,
    /// `size-adjust` face-а (CSS Fonts L4 §14.4): множитель `font-size`.
    pub(crate) size_adjust: f32,
}

/// Результат шейпинга под-run-а в единицах шрифта.
#[derive(Clone)]
struct RunUnits {
    text: Box<str>,
    /// Сумма `x_advance` всех глифов.
    advance: i64,
    /// Число кластеров: `letter-spacing` добавляется к каждому (CSS Text L3
    /// §8.2), а не к глифу. 0 — шейпер не принял face.
    clusters: u32,
}

thread_local! {
    static RUNS: RefCell<HashMap<u64, RunUnits>> = RefCell::new(HashMap::new());
}

fn run_key(fingerprint: u64, text: &str, features: &[([u8; 4], u32)], axes: &[([u8; 4], f32)]) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    fingerprint.hash(&mut h);
    text.hash(&mut h);
    features.hash(&mut h);
    for (tag, value) in axes {
        tag.hash(&mut h);
        value.to_bits().hash(&mut h);
    }
    h.finish()
}

/// `(сумма advance-ов, число кластеров)` run-а в единицах шрифта; `None` —
/// шейпер не принял face.
fn run_units(
    face: &ShapeFace<'_>,
    text: &str,
    features: &[([u8; 4], u32)],
    axes: &[([u8; 4], f32)],
) -> Option<(i64, u32)> {
    let key = run_key(face.fingerprint, text, features, axes);
    let hit = RUNS.with(|c| {
        c.borrow().get(&key).filter(|r| &*r.text == text).map(|r| (r.advance, r.clusters))
    });
    let (advance, clusters) = match hit {
        Some(h) => h,
        None => {
            let shaped = lumen_font::active_text_shaper().shape(
                face.bytes,
                text,
                lumen_core::ext::ShapeDirection::LeftToRight,
                None,
                features,
                axes,
            );
            let advance: i64 = shaped.iter().map(|g| i64::from(g.x_advance)).sum();
            // Кластер заканчивается там, где следующий глиф относится к другому.
            let clusters = shaped
                .iter()
                .enumerate()
                .filter(|(i, g)| shaped.get(i + 1).is_none_or(|n| n.cluster != g.cluster))
                .count() as u32;
            RUNS.with(|c| {
                let mut c = c.borrow_mut();
                if c.len() >= CACHE_MAX_ENTRIES {
                    c.clear();
                }
                c.insert(key, RunUnits { text: text.into(), advance, clusters });
            });
            (advance, clusters)
        }
    };
    (clusters > 0).then_some((advance, clusters))
}

/// Ширина под-run-а `text` в px: сумма advance-ов шейпера плюс
/// `letter-spacing` после каждого кластера — ровно то, на сколько нарисованный
/// run сдвигает перо. `None` — шейпер не принял face (вызывающий считает
/// посимвольно).
pub(crate) fn sub_run_width(face: &ShapeFace<'_>, text: &str, params: &ShapeParams<'_>) -> Option<f32> {
    if text.is_empty() {
        return Some(0.0);
    }
    if text_shaping_disabled() {
        return None;
    }
    let features = shaping_features(params.features, params.letter_spacing);
    let (advance, clusters) = run_units(face, text, &features, params.axes)?;
    let size = params.font_size_px * face.size_adjust;
    Some(advance as f32 * size / f32::from(face.units_per_em) + params.letter_spacing * clusters as f32)
}
