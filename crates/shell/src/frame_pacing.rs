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

/// Minimum interval between rAF batches (ms) — vsync gate at 60 Hz.
///
/// Prevents `requestAnimationFrame` from firing more than once per display frame
/// when `RedrawRequested` is delivered at higher frequency (e.g. from scroll events).
pub(crate) const RAF_MIN_INTERVAL_MS: f64 = 1000.0 / 60.0;

/// `true`, если fast-scroll деградация отключена
/// (`LUMEN_NO_FAST_SCROLL_DEGRADE=1`). Диагностика: A/B поведения и скорости
/// на одном бинарнике (паттерн `LUMEN_NO_SCROLL_COMPOSITOR`).
pub(crate) fn fast_scroll_degrade_disabled() -> bool {
    static DISABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *DISABLED.get_or_init(|| {
        std::env::var("LUMEN_NO_FAST_SCROLL_DEGRADE").is_ok_and(|v| v == "1")
    })
}

/// `true`, если фаст-пас страничного смещения отключён
/// (`LUMEN_NO_PAGE_OFFSET=1`) и шелл снова заворачивает display list в
/// `PushTransform` каждый кадр.
///
/// BUG-405 срез 38: рычаг заведён ради интерливед-A/B на ОДНОМ бинарнике
/// (`scripts/build_phase_census.py --arms offset`) — иначе плечи «до» и
/// «после» пришлось бы мерить разными сборками, что `docs/perf-method.md`
/// запрещает. Заодно это откат на случай, если фаст-пас вскроет дефект в
/// каком-то бэкенде.
pub(crate) fn page_offset_fast_disabled() -> bool {
    static DISABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *DISABLED.get_or_init(|| std::env::var("LUMEN_NO_PAGE_OFFSET").is_ok_and(|v| v == "1"))
}
