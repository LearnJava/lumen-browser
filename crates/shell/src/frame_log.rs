//! Renderer phase-log counters read once per frame (BUG-405 slices 34 and 37).
//!
//! The counters themselves live in the wgpu backend, so each accessor has a
//! `#[cfg(not(feature = "backend-wgpu"))]` stub next to it — femtovg prints no
//! phase block of its own.
//!
//! Moved out of `main.rs` by the SPLIT track (batch SH-3c); behaviour and
//! signatures are unchanged.

/// РќР°РЅРѕСЃРµРєСѓРЅРґС‹, РїРѕС‚СЂР°С‡РµРЅРЅС‹Рµ СЂРµРЅРґРµСЂРµСЂРѕРј РЅР° РџР•Р§РђРўР¬ РїРѕС„Р°Р·РЅРѕРіРѕ Р»РѕРіР° Р·Р° РїСЂРѕС†РµСЃСЃ
/// (BUG-405 СЃСЂРµР· 34). Р”РµР»СЊС‚Р° Р·Р° РєР°РґСЂ вЂ” С†РµРЅР° РёРЅСЃС‚СЂСѓРјРµРЅС‚Р° РІРЅСѓС‚СЂРё С„Р°Р·С‹ `paint`.
///
/// РЎС‡С‘С‚С‡РёРє Р¶РёРІС‘С‚ РІ wgpu-Р±СЌРєРµРЅРґРµ, РїРѕСЌС‚РѕРјСѓ Р±РµР· РЅРµРіРѕ СЃС‚Р°С‚СЊСЏ РїСѓСЃС‚Р°СЏ: femtovg СЃРІРѕРµРіРѕ
/// РїРѕС„Р°Р·РЅРѕРіРѕ Р±Р»РѕРєР° РЅРµ РїРµС‡Р°С‚Р°РµС‚.
#[cfg(feature = "backend-wgpu")]
pub(crate) fn frame_log_nanos() -> u64 {
    lumen_paint::load_counter(&lumen_paint::FRAME_LOG_NANOS)
}

/// РџРѕРґСЃС‚Р°С‚СЊРё РІС‹Р·РѕРІР° СЂРµРЅРґРµСЂРµСЂР° Р·Р° РїСЂРѕС†РµСЃСЃ, РјСЃ (BUG-405 СЃСЂРµР· 37): РїРѕРґРіРѕС‚РѕРІРєР°
/// РєРѕРјРїРѕРЅРѕРІРєРё, С…СЌС€ РєР°РґСЂР°, СЂРµС€РµРЅРёРµ РїРѕР»РѕСЃС‹, СЃСѓРјРјР° wgpu-РїР°СЃСЃРѕРІ.
///
/// Р”РµР»СЊС‚Р° Р·Р° РєР°РґСЂ СЂР°СЃРєР»Р°РґС‹РІР°РµС‚ СЃС‚Р°С‚СЊСЋ `paint` РЅР° РЈР РћР’РќР• 1 вЂ” РґРѕ СЃСЂРµР·Р° 37
/// СЂР°Р·Р±РёРІРєР° СЃСѓС‰РµСЃС‚РІРѕРІР°Р»Р° С‚РѕР»СЊРєРѕ РЅР° СѓСЂРѕРІРЅРµ 2, С‡СЊСЏ РїРµС‡Р°С‚СЊ РєСЂСѓРїРЅРµРµ СЃР°РјРѕРіРѕ РєР°РґСЂР°
/// РїРѕРїР°РґР°РЅРёСЏ (РїСѓРЅРєС‚ 71 РѕСЃС‚Р°С‚РєР°).
#[cfg(feature = "backend-wgpu")]
pub(crate) fn frame_phase_ms() -> [f64; 4] {
    std::array::from_fn(|i| {
        lumen_paint::FRAME_PHASE_NANOS
            .get(i)
            .map_or(0.0, |c| lumen_paint::load_counter(c) as f64 / 1e6)
    })
}

/// Р—Р°РіР»СѓС€РєР° [`frame_phase_ms`] РґР»СЏ СЃР±РѕСЂРєРё Р±РµР· wgpu-Р±СЌРєРµРЅРґР°.
#[cfg(not(feature = "backend-wgpu"))]
pub(crate) fn frame_phase_ms() -> [f64; 4] {
    [0.0; 4]
}

/// РњРµС‚РєР° РёСЃС…РѕРґР° РїСѓС‚Рё РєРѕРјРїРѕРЅРѕРІРєРё РЅР° РїРѕСЃР»РµРґРЅРµРј РєР°РґСЂРµ (BUG-405 СЃСЂРµР· 37) вЂ” РїРѕ РЅРµР№
/// РїРµСЂРµРїРёСЃСЊ РѕС‚Р±РёСЂР°РµС‚ РєР°РґСЂС‹ РџРћРџРђР”РђРќРРЇ, РЅРµ РїРѕРґРЅРёРјР°СЏ Р»РѕРі РґРѕ СѓСЂРѕРІРЅСЏ 2.
#[cfg(feature = "backend-wgpu")]
pub(crate) fn compose_outcome_label() -> &'static str {
    lumen_paint::last_compose().label()
}

/// Р—Р°РіР»СѓС€РєР° [`compose_outcome_label`] РґР»СЏ СЃР±РѕСЂРєРё Р±РµР· wgpu-Р±СЌРєРµРЅРґР°: РїСѓС‚СЊ
/// РєРѕРјРїРѕРЅРѕРІРєРё Р¶РёРІС‘С‚ С‚РѕР»СЊРєРѕ РІ wgpu-СЂРµРЅРґРµСЂРµСЂРµ, РєР°РґСЂРѕРІ РїРѕРїР°РґР°РЅРёСЏ С‚Р°Рј РЅРµ Р±С‹РІР°РµС‚.
#[cfg(not(feature = "backend-wgpu"))]
pub(crate) fn compose_outcome_label() -> &'static str {
    "-"
}

/// Р—Р°РіР»СѓС€РєР° [`frame_log_nanos`] РґР»СЏ СЃР±РѕСЂРєРё Р±РµР· wgpu-Р±СЌРєРµРЅРґР°.
#[cfg(not(feature = "backend-wgpu"))]
pub(crate) fn frame_log_nanos() -> u64 {
    0
}

/// Nanoseconds spent inside `render_with_anim` before its own `ComposeMarks`
/// timer starts (BUG-405 slice 44, `PRE_MARKS_NANOS`) — a candidate source of
/// the residual that survives even with the overlay cache disabled.
#[cfg(feature = "backend-wgpu")]
pub(crate) fn pre_marks_nanos() -> u64 {
    lumen_paint::load_counter(&lumen_paint::PRE_MARKS_NANOS)
}

/// Stub for [`pre_marks_nanos`] on non-wgpu builds.
#[cfg(not(feature = "backend-wgpu"))]
pub(crate) fn pre_marks_nanos() -> u64 {
    0
}

/// Nanoseconds spent inside `compose_page` between the `overlay_cache_step`
/// decision and the `render_impl` call (BUG-405 slice 44, `POST_CACHE_NANOS`)
/// — the second candidate п.84 itself names.
#[cfg(feature = "backend-wgpu")]
pub(crate) fn post_cache_nanos() -> u64 {
    lumen_paint::load_counter(&lumen_paint::POST_CACHE_NANOS)
}

/// Stub for [`post_cache_nanos`] on non-wgpu builds.
#[cfg(not(feature = "backend-wgpu"))]
pub(crate) fn post_cache_nanos() -> u64 {
    0
}

/// Nanoseconds spent inside `render_impl` between the `FRAME_PHASE_NANOS[3]`
/// snapshot and the function's own return (BUG-405 slice 45, `TAIL_NANOS`) —
/// the third candidate for the residual that survives п.84 even with slice
/// 44's two named candidates ruled out.
#[cfg(feature = "backend-wgpu")]
pub(crate) fn tail_nanos() -> u64 {
    lumen_paint::load_counter(&lumen_paint::TAIL_NANOS)
}

/// Stub for [`tail_nanos`] on non-wgpu builds.
#[cfg(not(feature = "backend-wgpu"))]
pub(crate) fn tail_nanos() -> u64 {
    0
}
