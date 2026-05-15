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
/// Соответствует §5.2 "discrete" правилу. Используется как fallback в
/// `LinearInterpolator` для типов, которые не поддерживают плавную
/// интерполяцию (Discrete-варианты, mixed Length units, и т.д.).
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

/// Реальная импл §5.2 — linear для Number / Length (same-unit) / Color
/// (RGBA sRGB, non-premultiplied alpha), step-half для Discrete и
/// для несовместимых пар (Length с разными unit-ами, Number ↔ Length,
/// и т.п.).
///
/// **Color interpolation** (CSS Color L4 §12.1): Phase 0 делает простой
/// per-component lerp в sRGB без premultiplied alpha. Spec рекомендует
/// premultiplied для корректного fade-out через
/// `rgba(255,0,0,1) → rgba(0,0,255,0)` (иначе появляются призрачные
/// сине-красные промежуточные кадры). Premultiplied-вариант — отдельная
/// задача (требует sRGB ↔ linear и обратно).
///
/// **Length interpolation** (CSS Values L4 §10): same-unit pairs
/// интерполируются скалярно; mixed-unit pairs по spec оборачиваются в
/// `calc(from*(1-t) + to*t)` — но в Phase 0 это потребовало бы построения
/// `CalcNode` с runtime-известным `t`. Используем step-half fallback
/// до момента, когда понадобится реально.
#[derive(Debug, Default, Clone, Copy)]
pub struct LinearInterpolator;

impl AnimationInterpolator for LinearInterpolator {
    fn interpolate(&self, from: &AnimValue, to: &AnimValue, t: f32) -> Option<AnimValue> {
        let t = t.clamp(0.0, 1.0);
        match (from, to) {
            (AnimValue::Number(a), AnimValue::Number(b)) => {
                Some(AnimValue::Number(lerp_f32(*a, *b, t)))
            }
            (AnimValue::Length(a), AnimValue::Length(b)) => interpolate_length(a, b, t)
                .map(AnimValue::Length)
                .or_else(|| Some(if t < 0.5 { from.clone() } else { to.clone() })),
            (AnimValue::Color(a), AnimValue::Color(b)) => {
                Some(AnimValue::Color(interpolate_color(*a, *b, t)))
            }
            _ => {
                // Discrete или mixed (Number ↔ Length / Length ↔ Color, и т.д.)
                // — §5.2 "discrete" step-half.
                Some(if t < 0.5 { from.clone() } else { to.clone() })
            }
        }
    }
}

#[inline]
fn lerp_f32(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

fn interpolate_length(from: &Length, to: &Length, t: f32) -> Option<Length> {
    match (from, to) {
        (Length::Px(a), Length::Px(b)) => Some(Length::Px(lerp_f32(*a, *b, t))),
        (Length::Em(a), Length::Em(b)) => Some(Length::Em(lerp_f32(*a, *b, t))),
        (Length::Rem(a), Length::Rem(b)) => Some(Length::Rem(lerp_f32(*a, *b, t))),
        (Length::Percent(a), Length::Percent(b)) => Some(Length::Percent(lerp_f32(*a, *b, t))),
        (Length::Vh(a), Length::Vh(b)) => Some(Length::Vh(lerp_f32(*a, *b, t))),
        (Length::Vw(a), Length::Vw(b)) => Some(Length::Vw(lerp_f32(*a, *b, t))),
        (Length::Vmin(a), Length::Vmin(b)) => Some(Length::Vmin(lerp_f32(*a, *b, t))),
        (Length::Vmax(a), Length::Vmax(b)) => Some(Length::Vmax(lerp_f32(*a, *b, t))),
        _ => None,
    }
}

fn interpolate_color(from: Color, to: Color, t: f32) -> Color {
    Color {
        r: lerp_u8(from.r, to.r, t),
        g: lerp_u8(from.g, to.g, t),
        b: lerp_u8(from.b, to.b, t),
        a: lerp_u8(from.a, to.a, t),
    }
}

#[inline]
fn lerp_u8(a: u8, b: u8, t: f32) -> u8 {
    let af = f32::from(a);
    let bf = f32::from(b);
    (af + (bf - af) * t).round().clamp(0.0, 255.0) as u8
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

    // ─────── LinearInterpolator ───────

    #[test]
    fn linear_number_at_endpoints() {
        let interp = LinearInterpolator;
        let from = AnimValue::Number(10.0);
        let to = AnimValue::Number(30.0);
        assert_eq!(interp.interpolate(&from, &to, 0.0), Some(from.clone()));
        assert_eq!(interp.interpolate(&from, &to, 1.0), Some(to.clone()));
    }

    #[test]
    fn linear_number_at_midpoint() {
        let interp = LinearInterpolator;
        let from = AnimValue::Number(10.0);
        let to = AnimValue::Number(30.0);
        assert_eq!(
            interp.interpolate(&from, &to, 0.5),
            Some(AnimValue::Number(20.0))
        );
    }

    #[test]
    fn linear_number_quarter() {
        let interp = LinearInterpolator;
        let from = AnimValue::Number(0.0);
        let to = AnimValue::Number(100.0);
        assert_eq!(
            interp.interpolate(&from, &to, 0.25),
            Some(AnimValue::Number(25.0))
        );
    }

    #[test]
    fn linear_clamps_t_out_of_range() {
        let interp = LinearInterpolator;
        let from = AnimValue::Number(10.0);
        let to = AnimValue::Number(20.0);
        assert_eq!(interp.interpolate(&from, &to, -1.0), Some(from.clone()));
        assert_eq!(interp.interpolate(&from, &to, 5.0), Some(to.clone()));
    }

    #[test]
    fn linear_length_same_unit_px() {
        let interp = LinearInterpolator;
        let from = AnimValue::Length(Length::Px(0.0));
        let to = AnimValue::Length(Length::Px(40.0));
        assert_eq!(
            interp.interpolate(&from, &to, 0.5),
            Some(AnimValue::Length(Length::Px(20.0)))
        );
    }

    #[test]
    fn linear_length_same_unit_percent() {
        let interp = LinearInterpolator;
        let from = AnimValue::Length(Length::Percent(0.0));
        let to = AnimValue::Length(Length::Percent(100.0));
        // f32 lerp может давать 30.000002 — сравниваем с допуском.
        match interp.interpolate(&from, &to, 0.3) {
            Some(AnimValue::Length(Length::Percent(v))) => {
                assert!((v - 30.0).abs() < 1e-3, "expected ~30.0, got {v}");
            }
            other => panic!("expected Length::Percent, got {other:?}"),
        }
    }

    #[test]
    fn linear_length_same_unit_em() {
        let interp = LinearInterpolator;
        let from = AnimValue::Length(Length::Em(1.0));
        let to = AnimValue::Length(Length::Em(2.0));
        assert_eq!(
            interp.interpolate(&from, &to, 0.5),
            Some(AnimValue::Length(Length::Em(1.5)))
        );
    }

    #[test]
    fn linear_length_mixed_unit_step_half() {
        let interp = LinearInterpolator;
        // Px ↔ Percent — несовместимы, fallback to step-half.
        let from = AnimValue::Length(Length::Px(10.0));
        let to = AnimValue::Length(Length::Percent(50.0));
        assert_eq!(interp.interpolate(&from, &to, 0.3), Some(from.clone()));
        assert_eq!(interp.interpolate(&from, &to, 0.7), Some(to.clone()));
    }

    #[test]
    fn linear_length_calc_step_half() {
        // Calc-стороны — step-half (interpolate_length вернёт None).
        let interp = LinearInterpolator;
        let from = AnimValue::Length(Length::Px(10.0));
        let to = AnimValue::Length(Length::Calc(Box::new(crate::style::CalcNode::Number(
            5.0,
        ))));
        assert_eq!(interp.interpolate(&from, &to, 0.3), Some(from.clone()));
        assert_eq!(interp.interpolate(&from, &to, 0.7), Some(to.clone()));
    }

    #[test]
    fn linear_color_at_endpoints() {
        let interp = LinearInterpolator;
        let from = AnimValue::Color(Color::BLACK);
        let to = AnimValue::Color(Color::WHITE);
        assert_eq!(interp.interpolate(&from, &to, 0.0), Some(from.clone()));
        assert_eq!(interp.interpolate(&from, &to, 1.0), Some(to.clone()));
    }

    #[test]
    fn linear_color_grey_at_half() {
        // (0,0,0,255) → (255,255,255,255) at t=0.5 = (128,128,128,255).
        let interp = LinearInterpolator;
        let from = AnimValue::Color(Color::BLACK);
        let to = AnimValue::Color(Color::WHITE);
        let mid = interp.interpolate(&from, &to, 0.5).unwrap();
        match mid {
            AnimValue::Color(c) => {
                // round-of-half — 128 (252/2 = 127.5 → 128).
                assert!(c.r == 128 || c.r == 127);
                assert_eq!(c.r, c.g);
                assert_eq!(c.g, c.b);
                assert_eq!(c.a, 255);
            }
            _ => panic!("expected Color"),
        }
    }

    #[test]
    fn linear_color_alpha_fades() {
        let interp = LinearInterpolator;
        let opaque = AnimValue::Color(Color {
            r: 255,
            g: 0,
            b: 0,
            a: 255,
        });
        let transparent = AnimValue::Color(Color {
            r: 255,
            g: 0,
            b: 0,
            a: 0,
        });
        let mid = interp.interpolate(&opaque, &transparent, 0.5).unwrap();
        match mid {
            AnimValue::Color(c) => {
                assert_eq!(c.r, 255);
                assert!(c.a == 128 || c.a == 127);
            }
            _ => panic!("expected Color"),
        }
    }

    #[test]
    fn linear_discrete_step_half() {
        let interp = LinearInterpolator;
        let from = AnimValue::Discrete("hidden".to_string());
        let to = AnimValue::Discrete("visible".to_string());
        assert_eq!(interp.interpolate(&from, &to, 0.3), Some(from.clone()));
        assert_eq!(interp.interpolate(&from, &to, 0.7), Some(to.clone()));
    }

    #[test]
    fn linear_incompatible_types_step_half() {
        // Number ↔ Color — несовместимая пара, step-half.
        let interp = LinearInterpolator;
        let from = AnimValue::Number(10.0);
        let to = AnimValue::Color(Color::BLACK);
        assert_eq!(interp.interpolate(&from, &to, 0.3), Some(from.clone()));
        assert_eq!(interp.interpolate(&from, &to, 0.7), Some(to.clone()));
    }
}
