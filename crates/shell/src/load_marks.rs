//! LOAD-0: метки загрузки страницы в живом окне.
//!
//! Три момента от старта навигации: первый кадр с контентом (потоковый
//! частичный DOM дал непустой display list), применение финальной страницы
//! (`apply_loaded_page`) и первый кадр после него. Отдельно от `nav_start`:
//! тот забирается в `RenderDone` для Navigation Timing, а метки живут до
//! финального кадра. Время передаётся параметром — логика проверяется без
//! часов.

use std::time::Instant;

/// Готовые метки, миллисекунды от старта навигации.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LoadTimings {
    pub first_content_ms: Option<f64>,
    pub final_applied_ms: f64,
    pub final_frame_ms: f64,
}

#[derive(Debug, Default)]
pub struct LoadMarks {
    t0: Option<Instant>,
    first_content: Option<Instant>,
    final_applied: Option<Instant>,
    /// Итог уже выдан — повторные кадры ничего не пишут.
    reported: bool,
}

impl LoadMarks {
    /// Новая навигация: всё сбрасывается, отсчёт идёт от `now`.
    pub fn reset(&mut self, now: Instant) {
        *self = Self { t0: Some(now), ..Self::default() };
    }

    /// Первый непустой display list потокового пути. Повторы игнорируются.
    pub fn mark_first_content(&mut self, now: Instant) {
        if self.t0.is_some() && self.first_content.is_none() && self.final_applied.is_none() {
            self.first_content = Some(now);
        }
    }

    /// Финальная страница применена.
    pub fn mark_final_applied(&mut self, now: Instant) {
        if self.t0.is_some() && self.final_applied.is_none() {
            self.final_applied = Some(now);
        }
    }

    /// Вызывается на каждом `RedrawRequested`. Первый кадр после применения
    /// финальной страницы возвращает итог (один раз за навигацию).
    pub fn mark_frame(&mut self, now: Instant) -> Option<LoadTimings> {
        let (t0, applied) = (self.t0?, self.final_applied?);
        if self.reported {
            return None;
        }
        self.reported = true;
        let ms = |t: Instant| t.saturating_duration_since(t0).as_secs_f64() * 1000.0;
        Some(LoadTimings {
            first_content_ms: self.first_content.map(ms),
            final_applied_ms: ms(applied),
            final_frame_ms: ms(now),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn at(t0: Instant, ms: u64) -> Instant {
        t0 + Duration::from_millis(ms)
    }

    #[test]
    fn fills_in_order_and_reports_once() {
        let t0 = Instant::now();
        let mut m = LoadMarks::default();
        m.reset(t0);
        assert!(m.mark_frame(at(t0, 5)).is_none(), "кадр до применения ничего не даёт");
        m.mark_first_content(at(t0, 100));
        m.mark_first_content(at(t0, 200)); // повтор не сдвигает метку
        m.mark_final_applied(at(t0, 900));
        let t = m.mark_frame(at(t0, 920)).expect("итог");
        assert_eq!(t.first_content_ms, Some(100.0));
        assert_eq!(t.final_applied_ms, 900.0);
        assert_eq!(t.final_frame_ms, 920.0);
        assert!(m.mark_frame(at(t0, 950)).is_none(), "второй кадр молчит");
    }

    #[test]
    fn no_streaming_paint_leaves_first_content_empty() {
        let t0 = Instant::now();
        let mut m = LoadMarks::default();
        m.reset(t0);
        m.mark_final_applied(at(t0, 300));
        let t = m.mark_frame(at(t0, 310)).unwrap();
        assert_eq!(t.first_content_ms, None);
    }

    #[test]
    fn reset_clears_previous_navigation() {
        let t0 = Instant::now();
        let mut m = LoadMarks::default();
        m.reset(t0);
        m.mark_first_content(at(t0, 10));
        m.mark_final_applied(at(t0, 20));
        assert!(m.mark_frame(at(t0, 30)).is_some());
        let t1 = at(t0, 1000);
        m.reset(t1);
        assert!(m.mark_frame(at(t1, 5)).is_none());
        m.mark_final_applied(at(t1, 40));
        let t = m.mark_frame(at(t1, 50)).unwrap();
        assert_eq!(t.first_content_ms, None);
        assert_eq!(t.final_applied_ms, 40.0);
    }

    #[test]
    fn marks_before_reset_are_ignored() {
        let mut m = LoadMarks::default();
        let now = Instant::now();
        m.mark_first_content(now);
        m.mark_final_applied(now);
        assert!(m.mark_frame(now).is_none());
    }

    #[test]
    fn first_content_after_final_is_ignored() {
        let t0 = Instant::now();
        let mut m = LoadMarks::default();
        m.reset(t0);
        m.mark_final_applied(at(t0, 50));
        m.mark_first_content(at(t0, 60));
        assert_eq!(m.mark_frame(at(t0, 70)).unwrap().first_content_ms, None);
    }
}
