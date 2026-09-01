//! Renderer phase-log counters read once per frame (BUG-405 slices 34 and 37).
//!
//! The counters themselves live in the wgpu backend, so each accessor has a
//! `#[cfg(not(feature = "backend-wgpu"))]` stub next to it — femtovg prints no
//! phase block of its own.
//!
//! Moved out of `main.rs` by the SPLIT track (batch SH-3c); behaviour and
//! signatures are unchanged.

/// Наносекунды, потраченные рендерером на ПЕЧАТЬ пофазного лога за процесс
/// (BUG-405 срез 34). Дельта за кадр — цена инструмента внутри фазы `paint`.
///
/// Счётчик живёт в wgpu-бэкенде, поэтому без него статья пустая: femtovg своего
/// пофазного блока не печатает.
#[cfg(feature = "backend-wgpu")]
pub(crate) fn frame_log_nanos() -> u64 {
    lumen_paint::load_counter(&lumen_paint::FRAME_LOG_NANOS)
}

/// Подстатьи вызова рендерера за процесс, мс (BUG-405 срез 37): подготовка
/// компоновки, хэш кадра, решение полосы, сумма wgpu-пассов.
///
/// Дельта за кадр раскладывает статью `paint` на УРОВНЕ 1 — до среза 37
/// разбивка существовала только на уровне 2, чья печать крупнее самого кадра
/// попадания (пункт 71 остатка).
#[cfg(feature = "backend-wgpu")]
pub(crate) fn frame_phase_ms() -> [f64; 4] {
    std::array::from_fn(|i| {
        lumen_paint::FRAME_PHASE_NANOS
            .get(i)
            .map_or(0.0, |c| lumen_paint::load_counter(c) as f64 / 1e6)
    })
}

/// Заглушка [`frame_phase_ms`] для сборки без wgpu-бэкенда.
#[cfg(not(feature = "backend-wgpu"))]
pub(crate) fn frame_phase_ms() -> [f64; 4] {
    [0.0; 4]
}

/// Метка исхода пути компоновки на последнем кадре (BUG-405 срез 37) — по ней
/// перепись отбирает кадры ПОПАДАНИЯ, не поднимая лог до уровня 2.
#[cfg(feature = "backend-wgpu")]
pub(crate) fn compose_outcome_label() -> &'static str {
    lumen_paint::last_compose().label()
}

/// Заглушка [`compose_outcome_label`] для сборки без wgpu-бэкенда: путь
/// компоновки живёт только в wgpu-рендерере, кадров попадания там не бывает.
#[cfg(not(feature = "backend-wgpu"))]
pub(crate) fn compose_outcome_label() -> &'static str {
    "-"
}

/// Заглушка [`frame_log_nanos`] для сборки без wgpu-бэкенда.
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
