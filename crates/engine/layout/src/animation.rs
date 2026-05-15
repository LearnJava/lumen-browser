//! Web Animations Level 1 — value interpolation (§5.2).
//!
//! Sprint 0 — контракт: trait `AnimationInterpolator` + `NoopInterpolator`
//! как stub. Реальная импл (cubic-bezier easing, типизированные value
//! pairs, invalidation animated-свойств) — P1 п.3A; compositor offload
//! для transform/opacity — P2 п.3B; scheduling в rendering steps stage —
//! P3 п.3B.
//!
//! Web Animations interpolation работает над парой `(from, to)` computed
//! values и параметром `t ∈ [0, 1]`. Для разных типов значений правила
//! отличаются (см. §5.2 spec):
//!
//! - **Числа / lengths** — линейная интерполяция `from + (to - from) * t`.
//! - **Colors** — RGBA-component интерполяция в sRGB или OKLab (CSS Color L4).
//! - **Transforms** — matrix-decompose / interpolate decomposed components /
//!   recompose; некоторые пары (translate↔matrix) требуют конверсии в
//!   matrix формы.
//! - **Discrete** — для non-interpolable пар (visibility, display): step-half
//!   (`t < 0.5` → from, иначе to).
//!
//! Sprint 0 stub имитирует discrete: всегда step-half, без типизации.

use crate::style::{Color, Length};

/// Анимируемое значение. Sprint 0: пять основных вариантов — Number /
/// Length / Color / Discrete (для non-interpolable свойств) / Vec<Length>
/// (для transform-list и shadow-списков).
///
/// Реальный список расширится в P1 п.3A: TransformList с
/// matrix-decompose, Filter с per-function интерполяцией, GradientStops,
/// Path-data для clip-path, и т.д.
#[derive(Debug, Clone, PartialEq)]
pub enum AnimValue {
    Number(f32),
    Length(Length),
    Color(Color),
    /// Дискретное (не-интерполируемое) значение — хранится как ключ:
    /// для interpolation просто step-half.
    Discrete(String),
}

/// Trait для интерполяции пары computed values.
///
/// Контракт:
/// - `t = 0.0` → возвращается значение, эквивалентное `from`.
/// - `t = 1.0` → возвращается значение, эквивалентное `to`.
/// - `0.0 < t < 1.0` — реализация выбирает: линейно (Number/Length/Color),
///   matrix-decompose (Transform — P1 п.3A), step-half (Discrete).
/// - `t < 0.0` или `t > 1.0` — поведение зависит от композитора: stub
///   принимает clamp к `[0, 1]`, реальные impl могут проигрывать
///   `fill: backwards/forwards`.
pub trait AnimationInterpolator {
    /// Интерполировать значение в точке `t`.
    ///
    /// Может вернуть `None`, если пара `(from, to)` несовместима (например,
    /// разные `AnimValue` варианты в строгой импл) — тогда композитор сам
    /// решает fallback (обычно — step-half).
    fn interpolate(&self, from: &AnimValue, to: &AnimValue, t: f32) -> Option<AnimValue>;
}

/// Stub-реализация: step-half для любой пары значений.
/// Соответствует §5.2 "discrete" правилу. Реальная импл (linear для
/// length/number/color, matrix-decompose для transform) — P1 п.3A.
#[derive(Debug, Default, Clone, Copy)]
pub struct NoopInterpolator;

impl AnimationInterpolator for NoopInterpolator {
    fn interpolate(&self, from: &AnimValue, to: &AnimValue, t: f32) -> Option<AnimValue> {
        let clamped = t.clamp(0.0, 1.0);
        Some(if clamped < 0.5 {
            from.clone()
        } else {
            to.clone()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn noop_returns_from_at_zero() {
        let interp = NoopInterpolator;
        let from = AnimValue::Number(10.0);
        let to = AnimValue::Number(20.0);
        assert_eq!(interp.interpolate(&from, &to, 0.0), Some(from));
    }

    #[test]
    fn noop_returns_to_at_one() {
        let interp = NoopInterpolator;
        let from = AnimValue::Number(10.0);
        let to = AnimValue::Number(20.0);
        assert_eq!(interp.interpolate(&from, &to, 1.0), Some(to));
    }

    #[test]
    fn noop_step_half_at_quarter() {
        let interp = NoopInterpolator;
        let from = AnimValue::Number(10.0);
        let to = AnimValue::Number(20.0);
        assert_eq!(
            interp.interpolate(&from, &to, 0.25),
            Some(AnimValue::Number(10.0))
        );
    }

    #[test]
    fn noop_step_half_at_three_quarters() {
        let interp = NoopInterpolator;
        let from = AnimValue::Number(10.0);
        let to = AnimValue::Number(20.0);
        assert_eq!(
            interp.interpolate(&from, &to, 0.75),
            Some(AnimValue::Number(20.0))
        );
    }

    #[test]
    fn noop_clamps_negative_t() {
        let interp = NoopInterpolator;
        let from = AnimValue::Color(Color::BLACK);
        let to = AnimValue::Color(Color::WHITE);
        assert_eq!(interp.interpolate(&from, &to, -0.5), Some(from));
    }

    #[test]
    fn noop_clamps_t_above_one() {
        let interp = NoopInterpolator;
        let from = AnimValue::Color(Color::BLACK);
        let to = AnimValue::Color(Color::WHITE);
        assert_eq!(interp.interpolate(&from, &to, 1.5), Some(to));
    }

    #[test]
    fn noop_works_with_length() {
        let interp = NoopInterpolator;
        let from = AnimValue::Length(Length::Px(0.0));
        let to = AnimValue::Length(Length::Px(100.0));
        assert_eq!(interp.interpolate(&from, &to, 0.49), Some(from.clone()));
        assert_eq!(interp.interpolate(&from, &to, 0.5), Some(to));
    }

    #[test]
    fn noop_works_with_discrete() {
        let interp = NoopInterpolator;
        let from = AnimValue::Discrete("hidden".to_string());
        let to = AnimValue::Discrete("visible".to_string());
        assert_eq!(interp.interpolate(&from, &to, 0.3), Some(from));
    }
}
