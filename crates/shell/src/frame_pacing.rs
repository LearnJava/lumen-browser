//! The three gates the redraw path consults once per frame: the 60 Hz vsync
//! gate that keeps `requestAnimationFrame` from firing twice for one display
//! frame, and the two `LUMEN_NO_*` levers that switch a fast path off so an
//! A/B measurement can be taken on one binary (BUG-405).
//!
//! Each lever is read once and cached in a `OnceLock`: the census arms flip
//! them per process, never mid-run.
//!
//! SPLIT-SH6 (2026-08-27): moved verbatim out of `main.rs`; only visibility
//! changed.

/// Minimum interval between rAF batches (ms) вЂ” vsync gate at 60 Hz.
///
/// Prevents `requestAnimationFrame` from firing more than once per display frame
/// when `RedrawRequested` is delivered at higher frequency (e.g. from scroll events).
pub(crate) const RAF_MIN_INTERVAL_MS: f64 = 1000.0 / 60.0;

/// `true`, РµСЃР»Рё fast-scroll РґРµРіСЂР°РґР°С†РёСЏ РѕС‚РєР»СЋС‡РµРЅР°
/// (`LUMEN_NO_FAST_SCROLL_DEGRADE=1`). Р”РёР°РіРЅРѕСЃС‚РёРєР°: A/B РїРѕРІРµРґРµРЅРёСЏ Рё СЃРєРѕСЂРѕСЃС‚Рё
/// РЅР° РѕРґРЅРѕРј Р±РёРЅР°СЂРЅРёРєРµ (РїР°С‚С‚РµСЂРЅ `LUMEN_NO_SCROLL_COMPOSITOR`).
pub(crate) fn fast_scroll_degrade_disabled() -> bool {
    static DISABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *DISABLED.get_or_init(|| {
        std::env::var("LUMEN_NO_FAST_SCROLL_DEGRADE").is_ok_and(|v| v == "1")
    })
}

/// `true`, РµСЃР»Рё С„Р°СЃС‚-РїР°СЃ СЃС‚СЂР°РЅРёС‡РЅРѕРіРѕ СЃРјРµС‰РµРЅРёСЏ РѕС‚РєР»СЋС‡С‘РЅ
/// (`LUMEN_NO_PAGE_OFFSET=1`) Рё С€РµР»Р» СЃРЅРѕРІР° Р·Р°РІРѕСЂР°С‡РёРІР°РµС‚ display list РІ
/// `PushTransform` РєР°Р¶РґС‹Р№ РєР°РґСЂ.
///
/// BUG-405 СЃСЂРµР· 38: СЂС‹С‡Р°Рі Р·Р°РІРµРґС‘РЅ СЂР°РґРё РёРЅС‚РµСЂР»РёРІРµРґ-A/B РЅР° РћР”РќРћРњ Р±РёРЅР°СЂРЅРёРєРµ
/// (`scripts/build_phase_census.py --arms offset`) вЂ” РёРЅР°С‡Рµ РїР»РµС‡Рё В«РґРѕВ» Рё
/// В«РїРѕСЃР»РµВ» РїСЂРёС€Р»РѕСЃСЊ Р±С‹ РјРµСЂРёС‚СЊ СЂР°Р·РЅС‹РјРё СЃР±РѕСЂРєР°РјРё, С‡С‚Рѕ `docs/perf-method.md`
/// Р·Р°РїСЂРµС‰Р°РµС‚. Р—Р°РѕРґРЅРѕ СЌС‚Рѕ РѕС‚РєР°С‚ РЅР° СЃР»СѓС‡Р°Р№, РµСЃР»Рё С„Р°СЃС‚-РїР°СЃ РІСЃРєСЂРѕРµС‚ РґРµС„РµРєС‚ РІ
/// РєР°РєРѕРј-С‚Рѕ Р±СЌРєРµРЅРґРµ.
pub(crate) fn page_offset_fast_disabled() -> bool {
    static DISABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *DISABLED.get_or_init(|| std::env::var("LUMEN_NO_PAGE_OFFSET").is_ok_and(|v| v == "1"))
}
