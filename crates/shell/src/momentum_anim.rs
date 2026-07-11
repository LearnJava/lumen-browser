//! Momentum (kinetic) scroll — плавное торможение после быстрого свайпа
//! на тачпаде. Запускается при `TouchPhase::Ended` с ненулевой скоростью
//! и тикается через `RedrawRequested` пока скорость не упадёт ниже порога.
//!
//! Физика: скорость убывает экспоненциально (v₀ · exp(−k·t)).
//! Смещение за интервал dt: Δp = v₀/k · (1 − exp(−k·dt)).
//! Полуатенюация — `HALF_LIFE_MS`: каждые 300 ms скорость вдвое меньше.
//! При |vy| + |vx| < `MIN_VELOCITY_PX_MS` анимация заканчивается.

/// Время (мс), за которое скорость падает вдвое. ~300 ms ≈ ощущение
/// Safari/Firefox на macOS при умеренном свайпе.
pub const HALF_LIFE_MS: f64 = 300.0;

/// Порог остановки: CSS px / ms. При 60 fps это ~3 px / frame — визуально
/// уже незаметно, тик прекращается.
pub const MIN_VELOCITY_PX_MS: f32 = 0.05;

/// Константа затухания, выведенная из `HALF_LIFE_MS`.
const DECAY_K: f64 = std::f64::consts::LN_2 / HALF_LIFE_MS;

/// Velocity-based momentum анимация. Хранится в `Lumen.momentum_anim`.
/// `advance()` вызывается перед рендером каждого кадра и возвращает
/// смещение, которое caller добавляет к scroll_y/scroll_x напрямую
/// (без `scroll_anim`, чтобы не конкурировать с keyboard smooth-scroll).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MomentumAnim {
    /// CSS px/ms — положительный = вниз.
    pub vel_y: f32,
    /// CSS px/ms — положительный = вправо.
    pub vel_x: f32,
    /// Timestamp последнего тика (ms от epoch shell-а).
    pub last_time_ms: f64,
}

impl MomentumAnim {
    pub fn new(vel_y: f32, vel_x: f32, now_ms: f64) -> Self {
        Self { vel_y, vel_x, last_time_ms: now_ms }
    }

    /// Прогнать анимацию до `now_ms`. Возвращает `(Δy, Δx, done)`.
    /// `done == true` — анимация завершилась, caller сбрасывает поле.
    /// Смещения в CSS px; caller обязан добавить их к scroll и clamp-нуть.
    pub fn advance(&mut self, now_ms: f64) -> (f32, f32, bool) {
        let dt = (now_ms - self.last_time_ms).max(0.0);
        self.last_time_ms = now_ms;

        if dt <= 0.0 {
            return (0.0, 0.0, false);
        }

        let exp_decay = (-DECAY_K * dt).exp() as f32;

        let dy = self.vel_y * (1.0 - exp_decay) / DECAY_K as f32;
        let dx = self.vel_x * (1.0 - exp_decay) / DECAY_K as f32;

        self.vel_y *= exp_decay;
        self.vel_x *= exp_decay;

        let done = self.vel_y.abs() + self.vel_x.abs() < MIN_VELOCITY_PX_MS;
        (dy, dx, done)
    }
}

/// Скорость момента в момент `t_ms`, если в `t0_ms` она была `v0`
/// (CSS px/ms). Чисто-функциональный сэмпл экспоненциального затухания
/// `v0 · exp(−k·Δt)` — без внутреннего состояния, поэтому одинаково
/// вычислим на UI- и на рендер-потоке (ADR-016 M1.3).
///
/// `Δt` клампится в `≥ 0`, так что «время в прошлом» возвращает `v0`.
pub fn velocity_at(v0: f32, t0_ms: f64, t_ms: f64) -> f32 {
    let dt = (t_ms - t0_ms).max(0.0);
    v0 * (-DECAY_K * dt).exp() as f32
}

/// Полное смещение (CSS px) от точки отсчёта за интервал `[t0_ms, t_ms]`,
/// если в `t0_ms` скорость равнялась `v0`: `Δp = v0/k · (1 − exp(−k·Δt))`.
///
/// Stateless-аналог [`MomentumAnim::advance`], но без побочных эффектов и без
/// зависимости от каденции тиков — рендер-поток (ADR-016 M1.3) продолжает
/// momentum из последнего закоммиченного кадра, вычисляя абсолютное смещение,
/// а не аккумулируя пошаговые дельты (иначе накапливался бы дрейф).
/// `Δt` клампится в `≥ 0`.
pub fn displacement_since(v0: f32, t0_ms: f64, t_ms: f64) -> f32 {
    let dt = (t_ms - t0_ms).max(0.0);
    if dt <= 0.0 {
        return 0.0;
    }
    let exp_decay = (-DECAY_K * dt).exp() as f32;
    v0 * (1.0 - exp_decay) / DECAY_K as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx(a: f32, b: f32) -> bool {
        (a - b).abs() < 0.5
    }

    #[test]
    fn zero_dt_gives_zero_displacement() {
        let mut m = MomentumAnim::new(1.0, 0.0, 1000.0);
        let (dy, dx, done) = m.advance(1000.0);
        assert!(approx(dy, 0.0));
        assert!(approx(dx, 0.0));
        assert!(!done);
    }

    #[test]
    fn velocity_decays_exponentially() {
        let mut m = MomentumAnim::new(1.0, 0.0, 0.0);
        m.advance(HALF_LIFE_MS);
        // После одного half-life скорость должна быть ~0.5.
        assert!((m.vel_y - 0.5).abs() < 0.01, "vel_y={}", m.vel_y);
    }

    #[test]
    fn two_half_lives_quarter_velocity() {
        let mut m = MomentumAnim::new(1.0, 0.0, 0.0);
        m.advance(HALF_LIFE_MS);
        m.advance(HALF_LIFE_MS * 2.0);
        assert!((m.vel_y - 0.25).abs() < 0.01, "vel_y={}", m.vel_y);
    }

    #[test]
    fn total_displacement_bounded() {
        // Полное смещение при v0=1 css px/ms: Δ_total = v0 / k = v0 * T_half / ln2.
        let v0 = 1.0_f32;
        let expected_total = v0 / DECAY_K as f32;
        let mut m = MomentumAnim::new(v0, 0.0, 0.0);
        let mut total = 0.0_f32;
        let mut t = 0.0_f64;
        loop {
            t += 16.0; // ~60 fps
            let (dy, _, done) = m.advance(t);
            total += dy;
            if done { break; }
            if t > 10_000.0 { panic!("не завершилась за 10 s"); }
        }
        // Ожидаем total ≈ expected_total (±5%).
        let diff = (total - expected_total).abs() / expected_total;
        assert!(diff < 0.05, "total={total:.1}, expected≈{expected_total:.1}");
    }

    #[test]
    fn animation_stops_below_threshold() {
        let mut m = MomentumAnim::new(0.04, 0.0, 0.0);
        // v0 уже ниже 2×MIN, должна сразу завершиться после первого тика.
        let (_, _, done) = m.advance(16.0);
        assert!(done);
    }

    #[test]
    fn negative_velocity_gives_negative_displacement() {
        let mut m = MomentumAnim::new(-2.0, 0.0, 0.0);
        let (dy, _, _) = m.advance(16.0);
        assert!(dy < 0.0, "dy={dy}");
    }

    #[test]
    fn x_and_y_independent() {
        let mut m = MomentumAnim::new(1.0, 2.0, 0.0);
        let (dy, dx, _) = m.advance(16.0);
        assert!(dy > 0.0);
        assert!(dx > 0.0);
        // dx должен быть примерно вдвое больше dy (v_x = 2×v_y).
        assert!((dx / dy - 2.0).abs() < 0.01, "dx/dy={}", dx / dy);
    }

    #[test]
    fn velocity_at_matches_half_life() {
        // Через один half-life скорость вдвое меньше.
        assert!((velocity_at(1.0, 0.0, HALF_LIFE_MS) - 0.5).abs() < 0.01);
        // «Время в прошлом» клампится — возвращает v0.
        assert_eq!(velocity_at(1.0, 100.0, 50.0), 1.0);
    }

    #[test]
    fn displacement_since_matches_stateful_advance() {
        // Stateless-сэмпл должен совпасть с суммой пошаговых advance() до 0.5%
        // — это гарант, что рендер-сторона (M1.3) не расходится с UI-стороной.
        let v0 = 1.5_f32;
        let mut m = MomentumAnim::new(v0, 0.0, 0.0);
        let mut stepwise = 0.0_f32;
        let mut t = 0.0_f64;
        for _ in 0..60 {
            t += 16.0;
            let (dy, _, _) = m.advance(t);
            stepwise += dy;
        }
        let stateless = displacement_since(v0, 0.0, t);
        let diff = (stepwise - stateless).abs() / stateless.abs();
        assert!(diff < 0.005, "stepwise={stepwise}, stateless={stateless}");
    }

    #[test]
    fn displacement_since_zero_dt_is_zero() {
        assert_eq!(displacement_since(2.0, 100.0, 100.0), 0.0);
        assert_eq!(displacement_since(2.0, 100.0, 50.0), 0.0);
    }

    #[test]
    fn displacement_since_total_bounded() {
        // За «бесконечное» время смещение стремится к v0/k.
        let v0 = 1.0_f32;
        let total = displacement_since(v0, 0.0, 1_000_000.0);
        let expected = v0 / (std::f64::consts::LN_2 / HALF_LIFE_MS) as f32;
        assert!((total - expected).abs() / expected < 0.001, "total={total}");
    }
}
