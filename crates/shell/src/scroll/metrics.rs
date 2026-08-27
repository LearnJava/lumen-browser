//! Scroll step sizes and clamping.
//!
//! The two empirical step constants/helpers the keyboard and scrollbar paths
//! share, plus the `scroll_y`/`scroll_x` clamp. Moved out of `main.rs` by the
//! SPLIT track (batch SH-5); behaviour and signatures are unchanged.

/// РЎРєРѕР»СЊРєРѕ CSS px СЃРєСЂРѕР»Р»РёРј Р·Р° СЃС‚СЂРµР»РєСѓ (line-step). Р­РјРїРёСЂРёС‡РµСЃРєРѕРµ Р·РЅР°С‡РµРЅРёРµ,
/// Р±Р»РёР·РєРѕРµ Рє Firefox/Chromium Р±РµР· smooth-scroll вЂ” РѕРєРѕР»Рѕ 2.5 СЃС‚СЂРѕРє 16-px С‚РµРєСЃС‚Р°.
pub(crate) const LINE_STEP_CSS_PX: f32 = 40.0;

/// PageDown / PageUp / Space вЂ” СЃРєРѕР»СЊРєРѕ РѕС‚ viewport-Р° Р·Р°С…РІР°С‚С‹РІР°РµРј Р·Р° РЅР°Р¶Р°С‚РёРµ.
/// РњРµРЅСЊС€Рµ 100% РґР°С‘С‚ overlap РјРµР¶РґСѓ В«СЃС‚СЂР°РЅРёС†Р°РјРёВ»: РїРѕР»СЊР·РѕРІР°С‚РµР»СЊ РЅРµ С‚РµСЂСЏРµС‚ РїРѕСЃР»РµРґРЅСЋСЋ
/// СЃС‚СЂРѕРєСѓ РёР· РІРёРґР°, С‡РёС‚Р°С‚СЊ РґР»РёРЅРЅС‹Рµ С‚РµРєСЃС‚С‹ РєРѕРјС„РѕСЂС‚РЅРµРµ.
pub(crate) fn page_step(viewport_height: f32) -> f32 {
    viewport_height * 0.9
}

/// РљР»Р°РјРї scroll_y РІ `[0, max]`. NaN-input в†’ 0 (Р·Р°С‰РёС‚Р° РѕС‚ arithmetic errors).
pub(crate) fn clamp_scroll(target: f32, max: f32) -> f32 {
    if target.is_nan() {
        return 0.0;
    }
    target.clamp(0.0, max)
}
