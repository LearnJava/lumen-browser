//! Scroll step sizes and clamping.
//!
//! The two empirical step constants/helpers the keyboard and scrollbar paths
//! share, plus the `scroll_y`/`scroll_x` clamp. Moved out of `main.rs` by the
//! SPLIT track (batch SH-5); behaviour and signatures are unchanged.

/// Сколько CSS px скроллим за стрелку (line-step). Эмпирическое значение,
/// близкое к Firefox/Chromium без smooth-scroll — около 2.5 строк 16-px текста.
pub(crate) const LINE_STEP_CSS_PX: f32 = 40.0;

/// PageDown / PageUp / Space — сколько от viewport-а захватываем за нажатие.
/// Меньше 100% даёт overlap между «страницами»: пользователь не теряет последнюю
/// строку из вида, читать длинные тексты комфортнее.
pub(crate) fn page_step(viewport_height: f32) -> f32 {
    viewport_height * 0.9
}

/// Кламп scroll_y в `[0, max]`. NaN-input → 0 (защита от arithmetic errors).
pub(crate) fn clamp_scroll(target: f32, max: f32) -> f32 {
    if target.is_nan() {
        return 0.0;
    }
    target.clamp(0.0, max)
}
