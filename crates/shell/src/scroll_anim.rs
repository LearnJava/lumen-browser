//! Smooth-scroll анимация `scroll_y`. Phase 0 — out-cubic easing
//! фиксированной длительности; tick-ается per `RedrawRequested`-frame
//! caller-ом (`Lumen::advance_scroll_anim`).
//!
//! Зачем: keyboard/wheel/page-jump раньше дёргали `scroll_y` мгновенно —
//! на длинных страницах теряется ощущение положения (текст «прыгает»).
//! Плавная анимация даёт пользователю context, какая часть viewport-а
//! заменилась.
//!
//! Drag thumb scrollbar-а намеренно остаётся instant — там visual feedback
//! даёт само движение мыши, а добавочная анимация ощущается как latency.

/// Длительность анимации scroll_y в миллисекундах. ~200 ms — компромисс
/// между «слишком быстро = ощущается как jump» и «слишком медленно =
/// раздражает». Firefox/Chromium используют похожие значения для
/// keyboard/wheel smooth-scroll preference.
pub const DURATION_MS: f64 = 200.0;

/// Снапшот анимации scroll_y. Хранится в `Lumen.scroll_anim`. Pure-данные —
/// никаких эффектов; sampling делается caller-ом по `now_ms` через
/// `sample(now_ms)`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScrollAnim {
    /// scroll_y в момент старта анимации (CSS px).
    pub start_y: f32,
    /// Куда едем (clamped в `[0, max_scroll]` на момент старта).
    pub target_y: f32,
    /// `DOMHighResTimeStamp` (ms от epoch shell-а) в момент старта.
    pub start_time_ms: f64,
}

impl ScrollAnim {
    /// Целевая точка анимации — для аддитивных вызовов
    /// (`scroll_by_smooth` поверх уже идущей анимации добавляет delta к
    /// target, чтобы repeat-input не «откатывал» в текущий scroll_y).
    pub fn target(&self) -> f32 {
        self.target_y
    }

    /// Posizione в момент `now_ms` (CSS px) и флаг завершения.
    ///
    /// До `start_time_ms` — отдаёт `start_y` (не двигаемся вспять во
    /// времени). После `start_time_ms + DURATION_MS` — возвращает
    /// `target_y` + `done=true`, caller обязан сбросить state.
    ///
    /// Easing — out-cubic: быстрый старт, плавное торможение. Стандарт
    /// для content scrolling; пользователь видит движение немедленно,
    /// затем оно затухает.
    pub fn sample(&self, now_ms: f64) -> (f32, bool) {
        let elapsed = now_ms - self.start_time_ms;
        if elapsed <= 0.0 {
            return (self.start_y, false);
        }
        if elapsed >= DURATION_MS {
            return (self.target_y, true);
        }
        let t = (elapsed / DURATION_MS) as f32;
        let eased = ease_out_cubic(t);
        let y = self.start_y + (self.target_y - self.start_y) * eased;
        (y, false)
    }
}

/// Out-cubic easing: `f(t) = 1 - (1-t)^3`. `f(0)=0`, `f(1)=1`. Параметр
/// клампится в `[0, 1]` — defense-in-depth, sample() уже это гарантирует.
pub fn ease_out_cubic(t: f32) -> f32 {
    let u = 1.0 - t.clamp(0.0, 1.0);
    1.0 - u * u * u
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-3
    }

    #[test]
    fn ease_endpoints() {
        assert!(approx(ease_out_cubic(0.0), 0.0));
        assert!(approx(ease_out_cubic(1.0), 1.0));
    }

    #[test]
    fn ease_is_monotonic() {
        let mut prev = ease_out_cubic(0.0);
        for i in 1..=100 {
            let t = i as f32 / 100.0;
            let v = ease_out_cubic(t);
            assert!(v >= prev, "не монотонна в t={t}: {prev} → {v}");
            prev = v;
        }
    }

    #[test]
    fn ease_clamps_outside_unit() {
        assert!(approx(ease_out_cubic(-0.5), 0.0));
        assert!(approx(ease_out_cubic(1.5), 1.0));
    }

    #[test]
    fn ease_midpoint_matches_formula() {
        // 1 - (1 - 0.5)^3 = 1 - 0.125 = 0.875
        assert!(approx(ease_out_cubic(0.5), 0.875));
    }

    fn anim(start: f32, target: f32, t0: f64) -> ScrollAnim {
        ScrollAnim {
            start_y: start,
            target_y: target,
            start_time_ms: t0,
        }
    }

    #[test]
    fn anim_sample_at_start_returns_start_y() {
        let a = anim(100.0, 500.0, 1000.0);
        let (y, done) = a.sample(1000.0);
        assert!(approx(y, 100.0));
        assert!(!done);
    }

    #[test]
    fn anim_sample_before_start_returns_start_y() {
        // Защита от обратного хода времени (системный clock skew / тесты).
        let a = anim(100.0, 500.0, 1000.0);
        let (y, done) = a.sample(500.0);
        assert!(approx(y, 100.0));
        assert!(!done);
    }

    #[test]
    fn anim_sample_at_end_returns_target_and_done() {
        let a = anim(100.0, 500.0, 1000.0);
        let (y, done) = a.sample(1000.0 + DURATION_MS);
        assert!(approx(y, 500.0));
        assert!(done);
    }

    #[test]
    fn anim_sample_past_end_clamps_to_target() {
        let a = anim(100.0, 500.0, 1000.0);
        let (y, done) = a.sample(5000.0);
        assert!(approx(y, 500.0));
        assert!(done);
    }

    #[test]
    fn anim_sample_midpoint_uses_ease_out_cubic() {
        let a = anim(0.0, 100.0, 0.0);
        let (y, done) = a.sample(DURATION_MS * 0.5);
        // ease_out_cubic(0.5) = 0.875 ⇒ 0 + 100 * 0.875 = 87.5
        assert!(approx(y, 87.5), "y = {y}, expected ~87.5");
        assert!(!done);
    }

    #[test]
    fn anim_sample_quarter_decelerates() {
        // ease_out_cubic(0.25) = 1 - 0.75^3 ≈ 0.578 ⇒ намного больше t=0.25.
        let a = anim(0.0, 100.0, 0.0);
        let (y, _) = a.sample(DURATION_MS * 0.25);
        assert!(y > 50.0, "out-cubic должен опередить linear; y={y}");
        assert!(y < 75.0, "out-cubic не должен скакать до 75%; y={y}");
    }

    #[test]
    fn anim_target_returns_target_y() {
        let a = anim(0.0, 250.0, 0.0);
        assert_eq!(a.target(), 250.0);
    }

    #[test]
    fn anim_backwards_animation_decelerates_too() {
        // start > target — анимация вверх. easing та же, монотонность сохраняется.
        let a = anim(500.0, 100.0, 0.0);
        let (y_quarter, _) = a.sample(DURATION_MS * 0.25);
        let (y_half, _) = a.sample(DURATION_MS * 0.5);
        let (y_end, done) = a.sample(DURATION_MS);
        assert!(y_quarter < 500.0, "должна начать двигаться к target");
        assert!(y_half < y_quarter, "монотонное снижение");
        assert!(approx(y_end, 100.0));
        assert!(done);
    }
}
