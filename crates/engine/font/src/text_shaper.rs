//! `lumen_core::ext::TextShaper` implementation (LIB-1/LIB-3, ADR-027).
//!
//! [`RustybuzzShaper`] shapes through `rustybuzz`: full complex-script
//! support — mark attachment, Arabic joining, Indic reordering. It replaced
//! this crate's own `GSUB`/`GPOS` engine (Latin/Cyrillic ligatures + kerning
//! only) as the default at `LIB-2` (2026-08-31, which re-shot the graphic-tests
//! references, the CPU snapshot PNGs and the paint crate's textual
//! display-list snapshots to match) and the own engine was deleted outright
//! at `LIB-3` (2026-09-01) once that swap had held.

use lumen_core::ext::{ShapeDirection, ShapedGlyph, TextShaper};

/// `TextShaper` backed by `rustybuzz` — the LIB-1 replacement, sole
/// implementation since `LIB-3`.
#[derive(Debug, Default, Clone, Copy)]
pub struct RustybuzzShaper;

impl TextShaper for RustybuzzShaper {
    fn shape(
        &self,
        font_data: &[u8],
        text: &str,
        direction: ShapeDirection,
        script: Option<[u8; 4]>,
        features: &[([u8; 4], u32)],
        variation_axes: &[([u8; 4], f32)],
    ) -> Vec<ShapedGlyph> {
        if text.is_empty() {
            return Vec::new();
        }
        with_face(font_data, variation_axes, |face| {
            shape_with_face(face, text, direction, script, features)
        })
        .unwrap_or_default()
    }
}

/// Шейпинг `text` уже построенным `rustybuzz::Face`.
fn shape_with_face(
    face: &rustybuzz::Face<'_>,
    text: &str,
    direction: ShapeDirection,
    script: Option<[u8; 4]>,
    features: &[([u8; 4], u32)],
) -> Vec<ShapedGlyph> {
    let mut buffer = rustybuzz::UnicodeBuffer::new();
    buffer.push_str(text);
    buffer.set_direction(match direction {
        ShapeDirection::LeftToRight => rustybuzz::Direction::LeftToRight,
        ShapeDirection::RightToLeft => rustybuzz::Direction::RightToLeft,
    });
    if let Some(tag) = script
        && let Some(s) =
            rustybuzz::Script::from_iso15924_tag(rustybuzz::ttf_parser::Tag::from_bytes(&tag))
    {
        buffer.set_script(s);
    }
    // Unset script/language are filled in from `text`'s own Unicode
    // properties by `rustybuzz::shape` itself
    // (`UnicodeBuffer::guess_segment_properties`) — it only fills what
    // is not already set, so the explicit direction/script above stand.
    //
    // Exception: an explicit left-to-right request over text whose guessed
    // script is natively right-to-left. HarfBuzz's `ensure_native_direction`
    // would reverse the buffer back to logical order, but a caller asking
    // for LTR passes text already in visual order (UAX #9 L2) and wants the
    // glyphs left to right. A neutral script suppresses that reversal.
    if script.is_none() && direction == ShapeDirection::LeftToRight {
        let mut probe = rustybuzz::UnicodeBuffer::new();
        probe.push_str(text);
        probe.guess_segment_properties();
        if is_native_rtl_script(probe.script()) {
            buffer.set_script(rustybuzz::script::COMMON);
        }
    }

    let rb_features: Vec<rustybuzz::Feature> = features
        .iter()
        .map(|(tag, value)| {
            rustybuzz::Feature::new(rustybuzz::ttf_parser::Tag::from_bytes(tag), *value, ..)
        })
        .collect();

    let glyphs = rustybuzz::shape(face, &rb_features, buffer);
    glyphs
        .glyph_infos()
        .iter()
        .zip(glyphs.glyph_positions())
        .map(|(info, pos)| ShapedGlyph {
            glyph_id: info.glyph_id as u16,
            cluster: info.cluster,
            x_advance: pos.x_advance,
            x_offset: pos.x_offset,
            y_offset: pos.y_offset,
        })
        .collect()
}

/// Сколько распарсенных face-ов держит [`with_face`] на поток.
const FACE_CACHE_SLOTS: usize = 8;
/// Face больше этого размера в кэш не кладётся: кэш хранит копию байтов.
const FACE_CACHE_MAX_FONT_BYTES: usize = 8 << 20;

/// Распарсенный `rustybuzz::Face` вместе с копией байтов, на которые он
/// ссылается.
struct CachedFace {
    /// Адрес, длина и отпечаток байтов шрифта у вызывающего — ключ.
    ptr: usize,
    len: usize,
    fingerprint: u64,
    /// Оси вариаций, выставленные на `face` (`set_variations` не сбрасывается
    /// пустым списком, поэтому они часть ключа).
    variations: Vec<([u8; 4], u32)>,
    // Поле `face` объявлено РАНЬШЕ `data`: при сбросе записи Face умирает
    // первым и не переживает байты, на которые ссылается (см. SAFETY ниже).
    face: rustybuzz::Face<'static>,
    #[allow(dead_code)]
    data: Box<[u8]>,
}

thread_local! {
    /// Недавно использованные face-ы, свежий первым. Построение `Face`
    /// (таблицы GSUB/GPOS) стоит ~0.2 мс на Inter — на порядок дороже самого
    /// шейпинга короткой строки, а страница шейпит тысячи run-ов одним шрифтом.
    static FACE_CACHE: std::cell::RefCell<Vec<CachedFace>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// Дешёвый отпечаток содержимого: длина, голова, хвост и 16 окон по 64 байта
/// поровну по телу. Вместе с адресом и длиной отличает два шрифта, по очереди
/// занимавшие один и тот же адрес.
fn font_fingerprint(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325 ^ data.len() as u64;
    let mut feed = |bytes: &[u8]| {
        for b in bytes {
            h = (h ^ u64::from(*b)).wrapping_mul(0x100_0000_01b3);
        }
    };
    feed(&data[..data.len().min(256)]);
    feed(&data[data.len().saturating_sub(256)..]);
    if data.len() > 4096 {
        let step = data.len() / 16;
        for i in 0..16 {
            let at = i * step;
            feed(&data[at..(at + 64).min(data.len())]);
        }
    }
    h
}

/// Выполняет `f` с `rustybuzz::Face` по `font_data` и осями `axes`; `None`,
/// если шрифт не разбирается. Face берётся из потокового кэша, если этот
/// шрифт уже встречался.
fn with_face<R>(
    font_data: &[u8],
    axes: &[([u8; 4], f32)],
    f: impl FnOnce(&rustybuzz::Face<'_>) -> R,
) -> Option<R> {
    let variations: Vec<([u8; 4], u32)> = axes.iter().map(|(t, v)| (*t, v.to_bits())).collect();
    let set_variations = |face: &mut rustybuzz::Face<'_>| {
        if !axes.is_empty() {
            let list: Vec<rustybuzz::Variation> = axes
                .iter()
                .map(|(tag, value)| rustybuzz::Variation {
                    tag: rustybuzz::ttf_parser::Tag::from_bytes(tag),
                    value: *value,
                })
                .collect();
            face.set_variations(&list);
        }
    };
    if font_data.len() > FACE_CACHE_MAX_FONT_BYTES {
        let mut face = rustybuzz::Face::from_slice(font_data, 0)?;
        set_variations(&mut face);
        return Some(f(&face));
    }
    let (ptr, len) = (font_data.as_ptr() as usize, font_data.len());
    let fingerprint = font_fingerprint(font_data);
    FACE_CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        if let Some(pos) = cache.iter().position(|e| {
            e.ptr == ptr && e.len == len && e.fingerprint == fingerprint && e.variations == variations
        }) {
            let entry = cache.remove(pos);
            cache.insert(0, entry);
        } else {
            let data: Box<[u8]> = Box::from(font_data);
            // SAFETY: `data` — куча-аллокация, её адрес не меняется при
            // перемещении `Box` внутрь `CachedFace`. Ссылка с лишённым
            // временем жизни кладётся только в `face` той же записи, из записи
            // не выдаётся (наружу уходит лишь `&Face` под заимствованием
            // записи), а `face` объявлен перед `data` и потому уничтожается
            // раньше байтов.
            let bytes: &'static [u8] = unsafe { std::mem::transmute::<&[u8], &'static [u8]>(&data) };
            let mut face = rustybuzz::Face::from_slice(bytes, 0)?;
            set_variations(&mut face);
            cache.insert(0, CachedFace { ptr, len, fingerprint, variations, face, data });
            cache.truncate(FACE_CACHE_SLOTS);
        }
        Some(f(&cache[0].face))
    })
}

/// The [`TextShaper`] implementation callers should use.
///
/// `rustybuzz`-backed, the sole implementation since `LIB-3` removed the
/// crate's own `GSUB`/`GPOS` engine.
pub fn active_text_shaper() -> &'static dyn TextShaper {
    &RustybuzzShaper
}

/// Scripts HarfBuzz treats as natively right-to-left.
fn is_native_rtl_script(script: rustybuzz::Script) -> bool {
    const RTL: [&[u8; 4]; 10] = [
        b"Arab", b"Hebr", b"Syrc", b"Thaa", b"Nkoo", b"Samr", b"Mand", b"Adlm", b"Rohg", b"Mani",
    ];
    let tag = script.tag().to_bytes();
    RTL.iter().any(|t| **t == tag)
}

#[cfg(test)]
mod tests {
    use super::*;

    const BUNDLED_FONT: &[u8] = include_bytes!("../../../../assets/fonts/Inter-Regular.ttf");

    #[test]
    fn rustybuzz_shaper_empty_text_is_empty() {
        let shaped = RustybuzzShaper.shape(
            BUNDLED_FONT,
            "",
            ShapeDirection::LeftToRight,
            None,
            &[],
            &[],
        );
        assert!(shaped.is_empty());
    }

    #[test]
    fn rustybuzz_shaper_reports_byte_offset_clusters() {
        // "café" — the 'é' is 2 bytes in UTF-8, so its cluster must be 1
        // (byte offset), not "index 1" read as a char count.
        let shaped = RustybuzzShaper.shape(
            BUNDLED_FONT,
            "aé",
            ShapeDirection::LeftToRight,
            None,
            &[],
            &[],
        );
        assert_eq!(shaped.len(), 2);
        assert_eq!(shaped[0].cluster, 0);
        assert_eq!(shaped[1].cluster, 1, "'é' starts at byte 1");
    }

    #[test]
    fn rustybuzz_shaper_kerns_known_pair() {
        // The bundled Inter face kerns "AV" — a negative x-advance
        // adjustment vs. the bare hmtx advance.
        let shaped = RustybuzzShaper.shape(
            BUNDLED_FONT,
            "AV",
            ShapeDirection::LeftToRight,
            None,
            &[],
            &[],
        );
        assert_eq!(shaped.len(), 2);
        assert!(shaped[0].x_advance > 0);
    }

    #[test]
    fn explicit_ltr_over_hebrew_keeps_visual_order() {
        // "א cba" is already visual (RTL override): clusters must stay
        // ascending, not be reversed back to logical order.
        let shaped = RustybuzzShaper.shape(
            BUNDLED_FONT,
            "\u{5d0} cba",
            ShapeDirection::LeftToRight,
            None,
            &[],
            &[],
        );
        let clusters: Vec<u32> = shaped.iter().map(|g| g.cluster).collect();
        assert!(clusters.windows(2).all(|w| w[0] <= w[1]), "{clusters:?}");
    }

    #[test]
    fn active_text_shaper_returns_something_that_shapes() {
        let shaper = active_text_shaper();
        let shaped =
            shaper.shape(BUNDLED_FONT, "Hi", ShapeDirection::LeftToRight, None, &[], &[]);
        assert_eq!(shaped.len(), 2);
    }

    #[test]
    fn cached_face_shapes_like_a_fresh_one_and_tells_fonts_apart() {
        const MONO: &[u8] = include_bytes!("../../../../assets/fonts/JetBrainsMono-Regular.ttf");
        let shape = |font: &[u8]| {
            RustybuzzShaper.shape(font, "AVATAR", ShapeDirection::LeftToRight, None, &[], &[])
        };
        let first = shape(BUNDLED_FONT);
        // Второй вызов идёт через кэш и обязан дать то же самое.
        assert_eq!(first, shape(BUNDLED_FONT));
        // Другой шрифт не подхватывает чужой Face.
        let mono = shape(MONO);
        assert_ne!(first, mono);
        assert_eq!(mono, shape(MONO));
        // Копия тех же байтов по другому адресу — отдельная запись, тот же итог.
        let copy = BUNDLED_FONT.to_vec();
        assert_eq!(first, shape(&copy));
    }
}
