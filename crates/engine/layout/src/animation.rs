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

use crate::style::{Color, Length, TransformFn};

/// Анимируемое значение. Phase 0: шесть вариантов — Number / Length / Color /
/// TransformList / Discrete (для non-interpolable свойств).
///
/// Реальный список расширится дальше: Filter с per-function интерполяцией,
/// GradientStops, Path-data для clip-path, и т.д.
#[derive(Debug, Clone, PartialEq)]
pub enum AnimValue {
    Number(f32),
    Length(Length),
    Color(Color),
    /// CSS Transforms L2 §15 — список transform-функций. Пустой Vec
    /// соответствует `transform: none`. Интерполяция делает matched-pair
    /// lerp для одинаковых по структуре списков и matrix-decompose для
    /// несовместимых пар.
    TransformList(Vec<TransformFn>),
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
            (AnimValue::TransformList(a), AnimValue::TransformList(b)) => Some(
                AnimValue::TransformList(interpolate_transform_list(a, b, t)),
            ),
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

// ─── Transform-list interpolation (CSS Transforms L2 §15) ───────────────────

/// Интерполяция transform-list по CSS Transforms Level 2 §15:
/// - Если `from` и `to` идентичны по структуре (длина + те же варианты
///   `TransformFn` в тех же позициях) — matched-pair lerp каждой функции.
/// - Иначе — обе стороны композируются в 2D-аффинную матрицу, та
///   декомпозируется в (translate, rotate, scale, skew), компоненты
///   интерполируются и recompose в один `TransformFn::Matrix(...)`.
///
/// Пустой `Vec` соответствует `transform: none` — трактуется как identity
/// при matrix-fallback пути (нулевые translate / нулевая rotation /
/// единичный scale / нулевой skew).
pub(crate) fn interpolate_transform_list(
    from: &[TransformFn],
    to: &[TransformFn],
    t: f32,
) -> Vec<TransformFn> {
    // Обе стороны пусты — `none → none`, ничего не анимируется.
    if from.is_empty() && to.is_empty() {
        return Vec::new();
    }

    if transform_lists_match_kind(from, to) {
        return from
            .iter()
            .zip(to.iter())
            .map(|(a, b)| interpolate_transform_fn_same_kind(a, b, t))
            .collect();
    }

    // Fallback: matrix decompose path.
    let from_m = compose_2d_affine(from);
    let to_m = compose_2d_affine(to);
    let from_d = decompose_2d_affine(from_m);
    let to_d = decompose_2d_affine(to_m);
    let interp_d = interpolate_decomposed(from_d, to_d, t);
    let m = recompose_2d_affine(interp_d);
    vec![TransformFn::Matrix(m)]
}

fn transform_lists_match_kind(a: &[TransformFn], b: &[TransformFn]) -> bool {
    if a.len() != b.len() || a.is_empty() {
        return false;
    }
    a.iter()
        .zip(b.iter())
        .all(|(x, y)| same_transform_kind(x, y))
}

fn same_transform_kind(a: &TransformFn, b: &TransformFn) -> bool {
    use TransformFn::*;
    matches!(
        (a, b),
        (Translate(..), Translate(..))
            | (TranslateX(_), TranslateX(_))
            | (TranslateY(_), TranslateY(_))
            | (Rotate(_), Rotate(_))
            | (Scale(..), Scale(..))
            | (ScaleX(_), ScaleX(_))
            | (ScaleY(_), ScaleY(_))
            | (SkewX(_), SkewX(_))
            | (SkewY(_), SkewY(_))
            | (Matrix(_), Matrix(_))
    )
}

/// Per-function lerp при одинаковом TransformFn variant. Контракт: вызывается
/// только когда `same_transform_kind(a, b)` истинно. `Rotate(α) → Rotate(β)`
/// — линейная интерполяция угла без shortest-path: CSS требует, чтобы
/// `rotate(0deg) → rotate(720deg)` действительно прокручивал два оборота.
/// Shortest-path работает только в matrix-fallback (там mod 2π).
fn interpolate_transform_fn_same_kind(a: &TransformFn, b: &TransformFn, t: f32) -> TransformFn {
    use TransformFn::*;
    match (a, b) {
        (Translate(ax, ay), Translate(bx, by)) => {
            Translate(lerp_f32(*ax, *bx, t), lerp_f32(*ay, *by, t))
        }
        (TranslateX(a), TranslateX(b)) => TranslateX(lerp_f32(*a, *b, t)),
        (TranslateY(a), TranslateY(b)) => TranslateY(lerp_f32(*a, *b, t)),
        (Rotate(a), Rotate(b)) => Rotate(lerp_f32(*a, *b, t)),
        (Scale(ax, ay), Scale(bx, by)) => Scale(lerp_f32(*ax, *bx, t), lerp_f32(*ay, *by, t)),
        (ScaleX(a), ScaleX(b)) => ScaleX(lerp_f32(*a, *b, t)),
        (ScaleY(a), ScaleY(b)) => ScaleY(lerp_f32(*a, *b, t)),
        (SkewX(a), SkewX(b)) => SkewX(lerp_f32(*a, *b, t)),
        (SkewY(a), SkewY(b)) => SkewY(lerp_f32(*a, *b, t)),
        (Matrix(a), Matrix(b)) => {
            // Две Matrix-функции интерполируем через decompose path —
            // покомпонентный lerp шести чисел даёт визуально неверный
            // результат (например, сжатие до нуля посередине анимации
            // pure-rotation 0° → 180°).
            let from_d = decompose_2d_affine(*a);
            let to_d = decompose_2d_affine(*b);
            let interp_d = interpolate_decomposed(from_d, to_d, t);
            Matrix(recompose_2d_affine(interp_d))
        }
        // Все остальные сочетания фильтрует `same_transform_kind` — сюда
        // никогда не приходим. Заглушка только для exhaustiveness.
        _ => a.clone(),
    }
}

/// 2D-аффинная матрица как `[a, b, c, d, e, f]` (CSS Transforms L1 §13.10):
/// `x' = a·x + c·y + e`, `y' = b·x + d·y + f`.
type Affine = [f32; 6];

const IDENTITY: Affine = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];

fn mul_affine(lhs: Affine, rhs: Affine) -> Affine {
    let [a1, b1, c1, d1, e1, f1] = lhs;
    let [a2, b2, c2, d2, e2, f2] = rhs;
    // Композиция «lhs затем rhs применительно к точке» = lhs * rhs.
    // p' = lhs * (rhs * p). Тождественно с Mat4::multiply в property_trees.
    [
        a1 * a2 + c1 * b2,
        b1 * a2 + d1 * b2,
        a1 * c2 + c1 * d2,
        b1 * c2 + d1 * d2,
        a1 * e2 + c1 * f2 + e1,
        b1 * e2 + d1 * f2 + f1,
    ]
}

fn affine_of(fn_: &TransformFn) -> Affine {
    match fn_ {
        TransformFn::Translate(x, y) => [1.0, 0.0, 0.0, 1.0, *x, *y],
        TransformFn::TranslateX(x) => [1.0, 0.0, 0.0, 1.0, *x, 0.0],
        TransformFn::TranslateY(y) => [1.0, 0.0, 0.0, 1.0, 0.0, *y],
        TransformFn::Rotate(theta) => {
            let c = theta.cos();
            let s = theta.sin();
            [c, s, -s, c, 0.0, 0.0]
        }
        TransformFn::Scale(sx, sy) => [*sx, 0.0, 0.0, *sy, 0.0, 0.0],
        TransformFn::ScaleX(sx) => [*sx, 0.0, 0.0, 1.0, 0.0, 0.0],
        TransformFn::ScaleY(sy) => [1.0, 0.0, 0.0, *sy, 0.0, 0.0],
        TransformFn::SkewX(a) => [1.0, 0.0, a.tan(), 1.0, 0.0, 0.0],
        TransformFn::SkewY(a) => [1.0, a.tan(), 0.0, 1.0, 0.0, 0.0],
        TransformFn::Matrix(m) => *m,
    }
}

fn compose_2d_affine(fns: &[TransformFn]) -> Affine {
    fns.iter()
        .fold(IDENTITY, |acc, f| mul_affine(acc, affine_of(f)))
}

/// Декомпозиция 2D-аффинной матрицы в (translate, rotate, scale, skew)
/// по CSS Transforms Level 2 §15.6. Skew хранится как тангенс угла —
/// это согласовано с recompose, где он применяется как `skewX(atan(sk))`.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Decomposed2D {
    tx: f32,
    ty: f32,
    scale_x: f32,
    scale_y: f32,
    skew: f32,
    rotation: f32,
}

const IDENTITY_DECOMP: Decomposed2D = Decomposed2D {
    tx: 0.0,
    ty: 0.0,
    scale_x: 1.0,
    scale_y: 1.0,
    skew: 0.0,
    rotation: 0.0,
};

fn decompose_2d_affine(m: Affine) -> Decomposed2D {
    let [mut a, mut b, mut c, mut d, e, f] = m;

    // Сингулярная матрица — вернуть identity-decomp, чтобы recompose дал
    // что-то сенсорно похожее (идемпотентная заглушка).
    let det = a * d - b * c;
    if det.abs() < f32::EPSILON {
        return IDENTITY_DECOMP;
    }

    let mut scale_x = (a * a + b * b).sqrt();
    if scale_x > 0.0 {
        a /= scale_x;
        b /= scale_x;
    }

    let mut skew = a * c + b * d;
    c -= a * skew;
    d -= b * skew;

    let scale_y = (c * c + d * d).sqrt();
    if scale_y > 0.0 {
        skew /= scale_y;
    }

    // Отражение: если determinant отрицательный — флипнуть scale_x и r0.
    // Знак b после `a.atan2()` определит rotation; b обновляем тоже.
    if det < 0.0 {
        scale_x = -scale_x;
        a = -a;
        b = -b;
    }

    Decomposed2D {
        tx: e,
        ty: f,
        scale_x,
        scale_y,
        skew,
        rotation: b.atan2(a),
    }
}

fn recompose_2d_affine(d: Decomposed2D) -> Affine {
    let cos = d.rotation.cos();
    let sin = d.rotation.sin();
    // M = T(tx,ty) * R(θ) * Skew(sk) * S(scale_x, scale_y).
    let a = cos * d.scale_x;
    let b = sin * d.scale_x;
    let c = (cos * d.skew - sin) * d.scale_y;
    let dd = (sin * d.skew + cos) * d.scale_y;
    [a, b, c, dd, d.tx, d.ty]
}

fn interpolate_decomposed(from: Decomposed2D, to: Decomposed2D, t: f32) -> Decomposed2D {
    // Shortest-path для rotation (только в matrix-fallback path).
    let mut diff = to.rotation - from.rotation;
    let two_pi = std::f32::consts::TAU;
    while diff > std::f32::consts::PI {
        diff -= two_pi;
    }
    while diff < -std::f32::consts::PI {
        diff += two_pi;
    }
    Decomposed2D {
        tx: lerp_f32(from.tx, to.tx, t),
        ty: lerp_f32(from.ty, to.ty, t),
        scale_x: lerp_f32(from.scale_x, to.scale_x, t),
        scale_y: lerp_f32(from.scale_y, to.scale_y, t),
        skew: lerp_f32(from.skew, to.skew, t),
        rotation: from.rotation + diff * t,
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

    // ─── TransformList interpolation ───────────────────────────────────────

    fn approx_eq(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-3
    }

    fn assert_affine_close(actual: [f32; 6], expected: [f32; 6]) {
        for i in 0..6 {
            assert!(
                approx_eq(actual[i], expected[i]),
                "affine[{i}]: expected {}, got {} (full actual = {:?})",
                expected[i],
                actual[i],
                actual
            );
        }
    }

    #[test]
    fn linear_transform_list_empty_pair_stays_empty() {
        let interp = LinearInterpolator;
        let from = AnimValue::TransformList(Vec::new());
        let to = AnimValue::TransformList(Vec::new());
        match interp.interpolate(&from, &to, 0.5).unwrap() {
            AnimValue::TransformList(v) => assert!(v.is_empty()),
            _ => panic!("expected TransformList"),
        }
    }

    #[test]
    fn linear_transform_matched_translate_at_midpoint() {
        let interp = LinearInterpolator;
        let from = AnimValue::TransformList(vec![TransformFn::Translate(0.0, 0.0)]);
        let to = AnimValue::TransformList(vec![TransformFn::Translate(40.0, 80.0)]);
        match interp.interpolate(&from, &to, 0.5).unwrap() {
            AnimValue::TransformList(v) => {
                assert_eq!(v.len(), 1);
                match v[0] {
                    TransformFn::Translate(x, y) => {
                        assert!(approx_eq(x, 20.0) && approx_eq(y, 40.0));
                    }
                    _ => panic!("expected Translate"),
                }
            }
            _ => panic!("expected TransformList"),
        }
    }

    #[test]
    fn linear_transform_matched_translatex() {
        let interp = LinearInterpolator;
        let from = AnimValue::TransformList(vec![TransformFn::TranslateX(10.0)]);
        let to = AnimValue::TransformList(vec![TransformFn::TranslateX(50.0)]);
        match interp.interpolate(&from, &to, 0.25).unwrap() {
            AnimValue::TransformList(v) => match v[0] {
                TransformFn::TranslateX(x) => assert!(approx_eq(x, 20.0)),
                _ => panic!("expected TranslateX"),
            },
            _ => panic!("expected TransformList"),
        }
    }

    #[test]
    fn linear_transform_matched_rotate_no_shortest_path() {
        // CSS Transforms L2 §15: matched-pair rotate — линейный lerp угла,
        // 0 → 720° анимирует два оборота.
        let interp = LinearInterpolator;
        let from = AnimValue::TransformList(vec![TransformFn::Rotate(0.0)]);
        let to = AnimValue::TransformList(vec![TransformFn::Rotate(std::f32::consts::TAU * 2.0)]);
        match interp.interpolate(&from, &to, 0.25).unwrap() {
            AnimValue::TransformList(v) => match v[0] {
                TransformFn::Rotate(a) => {
                    let expected = std::f32::consts::TAU * 0.5;
                    assert!(approx_eq(a, expected), "got {a}, expected {expected}");
                }
                _ => panic!("expected Rotate"),
            },
            _ => panic!("expected TransformList"),
        }
    }

    #[test]
    fn linear_transform_matched_scale() {
        let interp = LinearInterpolator;
        let from = AnimValue::TransformList(vec![TransformFn::Scale(1.0, 1.0)]);
        let to = AnimValue::TransformList(vec![TransformFn::Scale(3.0, 5.0)]);
        match interp.interpolate(&from, &to, 0.5).unwrap() {
            AnimValue::TransformList(v) => match v[0] {
                TransformFn::Scale(x, y) => {
                    assert!(approx_eq(x, 2.0) && approx_eq(y, 3.0));
                }
                _ => panic!("expected Scale"),
            },
            _ => panic!("expected TransformList"),
        }
    }

    #[test]
    fn linear_transform_matched_skewx() {
        let interp = LinearInterpolator;
        let from = AnimValue::TransformList(vec![TransformFn::SkewX(0.0)]);
        let to = AnimValue::TransformList(vec![TransformFn::SkewX(std::f32::consts::FRAC_PI_2)]);
        match interp.interpolate(&from, &to, 0.5).unwrap() {
            AnimValue::TransformList(v) => match v[0] {
                TransformFn::SkewX(a) => {
                    assert!(approx_eq(a, std::f32::consts::FRAC_PI_4));
                }
                _ => panic!("expected SkewX"),
            },
            _ => panic!("expected TransformList"),
        }
    }

    #[test]
    fn linear_transform_matched_multi_function_list() {
        // [translate, scale] → [translate, scale] — обе функции lerp-ятся
        // независимо.
        let interp = LinearInterpolator;
        let from = AnimValue::TransformList(vec![
            TransformFn::Translate(0.0, 0.0),
            TransformFn::Scale(1.0, 1.0),
        ]);
        let to = AnimValue::TransformList(vec![
            TransformFn::Translate(100.0, 0.0),
            TransformFn::Scale(2.0, 2.0),
        ]);
        match interp.interpolate(&from, &to, 0.5).unwrap() {
            AnimValue::TransformList(v) => {
                assert_eq!(v.len(), 2);
                match v[0] {
                    TransformFn::Translate(x, y) => {
                        assert!(approx_eq(x, 50.0) && approx_eq(y, 0.0));
                    }
                    _ => panic!("expected Translate"),
                }
                match v[1] {
                    TransformFn::Scale(x, y) => {
                        assert!(approx_eq(x, 1.5) && approx_eq(y, 1.5));
                    }
                    _ => panic!("expected Scale"),
                }
            }
            _ => panic!("expected TransformList"),
        }
    }

    #[test]
    fn linear_transform_mismatched_kinds_uses_matrix_decompose() {
        // [translate(100)] → [scale(2)] — длина равна, но варианты разные.
        // Падаем в matrix decompose, ожидаем единственный Matrix-результат.
        let interp = LinearInterpolator;
        let from = AnimValue::TransformList(vec![TransformFn::Translate(100.0, 0.0)]);
        let to = AnimValue::TransformList(vec![TransformFn::Scale(2.0, 2.0)]);
        match interp.interpolate(&from, &to, 0.5).unwrap() {
            AnimValue::TransformList(v) => {
                assert_eq!(v.len(), 1);
                assert!(matches!(v[0], TransformFn::Matrix(_)));
            }
            _ => panic!("expected TransformList"),
        }
    }

    #[test]
    fn linear_transform_mismatched_length_uses_matrix_decompose() {
        let interp = LinearInterpolator;
        let from = AnimValue::TransformList(vec![TransformFn::Translate(10.0, 20.0)]);
        let to = AnimValue::TransformList(vec![
            TransformFn::Translate(30.0, 40.0),
            TransformFn::Rotate(0.0),
        ]);
        match interp.interpolate(&from, &to, 0.5).unwrap() {
            AnimValue::TransformList(v) => {
                assert_eq!(v.len(), 1);
                assert!(matches!(v[0], TransformFn::Matrix(_)));
            }
            _ => panic!("expected TransformList"),
        }
    }

    #[test]
    fn linear_transform_none_to_translate_uses_matrix_decompose() {
        // none → translateX(100px) → matrix path с identity-from.
        let interp = LinearInterpolator;
        let from = AnimValue::TransformList(Vec::new());
        let to = AnimValue::TransformList(vec![TransformFn::TranslateX(100.0)]);
        match interp.interpolate(&from, &to, 0.5).unwrap() {
            AnimValue::TransformList(v) => {
                assert_eq!(v.len(), 1);
                match v[0] {
                    TransformFn::Matrix([a, b, c, d, e, f]) => {
                        // identity-rotate / identity-scale / нулевой skew —
                        // только translate.x при t=0.5 = 50.
                        assert!(approx_eq(a, 1.0) && approx_eq(b, 0.0));
                        assert!(approx_eq(c, 0.0) && approx_eq(d, 1.0));
                        assert!(approx_eq(e, 50.0) && approx_eq(f, 0.0));
                    }
                    _ => panic!("expected Matrix"),
                }
            }
            _ => panic!("expected TransformList"),
        }
    }

    #[test]
    fn linear_transform_matrix_pair_uses_decompose() {
        // Две Matrix-функции — заходят на decompose path внутри matched-pair.
        // matrix(1,0,0,1,0,0) (identity) → matrix(1,0,0,1,200,0) (translate 200,0)
        // at t=0.5 должен дать матрицу translate(100,0).
        let interp = LinearInterpolator;
        let from = AnimValue::TransformList(vec![TransformFn::Matrix(IDENTITY)]);
        let to = AnimValue::TransformList(vec![TransformFn::Matrix([
            1.0, 0.0, 0.0, 1.0, 200.0, 0.0,
        ])]);
        match interp.interpolate(&from, &to, 0.5).unwrap() {
            AnimValue::TransformList(v) => {
                assert_eq!(v.len(), 1);
                match v[0] {
                    TransformFn::Matrix(m) => {
                        assert_affine_close(m, [1.0, 0.0, 0.0, 1.0, 100.0, 0.0]);
                    }
                    _ => panic!("expected Matrix"),
                }
            }
            _ => panic!("expected TransformList"),
        }
    }

    #[test]
    fn linear_transform_endpoint_t_zero_returns_from() {
        let interp = LinearInterpolator;
        let from = AnimValue::TransformList(vec![TransformFn::Translate(10.0, 20.0)]);
        let to = AnimValue::TransformList(vec![TransformFn::Translate(30.0, 40.0)]);
        // matched-pair lerp с t=0 → ровно from.
        match interp.interpolate(&from, &to, 0.0).unwrap() {
            AnimValue::TransformList(v) => match v[0] {
                TransformFn::Translate(x, y) => {
                    assert!(approx_eq(x, 10.0) && approx_eq(y, 20.0));
                }
                _ => panic!("expected Translate"),
            },
            _ => panic!("expected TransformList"),
        }
    }

    #[test]
    fn linear_transform_endpoint_t_one_returns_to() {
        let interp = LinearInterpolator;
        let from = AnimValue::TransformList(vec![TransformFn::Translate(10.0, 20.0)]);
        let to = AnimValue::TransformList(vec![TransformFn::Translate(30.0, 40.0)]);
        match interp.interpolate(&from, &to, 1.0).unwrap() {
            AnimValue::TransformList(v) => match v[0] {
                TransformFn::Translate(x, y) => {
                    assert!(approx_eq(x, 30.0) && approx_eq(y, 40.0));
                }
                _ => panic!("expected Translate"),
            },
            _ => panic!("expected TransformList"),
        }
    }

    // ─── Decompose / recompose round-trip ─────────────────────────────────

    #[test]
    fn decompose_identity_round_trip() {
        let d = decompose_2d_affine(IDENTITY);
        let m = recompose_2d_affine(d);
        assert_affine_close(m, IDENTITY);
    }

    #[test]
    fn decompose_pure_translate_round_trip() {
        let m = [1.0, 0.0, 0.0, 1.0, 30.0, 40.0];
        let d = decompose_2d_affine(m);
        assert!(approx_eq(d.tx, 30.0) && approx_eq(d.ty, 40.0));
        assert!(approx_eq(d.scale_x, 1.0) && approx_eq(d.scale_y, 1.0));
        assert!(approx_eq(d.skew, 0.0) && approx_eq(d.rotation, 0.0));
        assert_affine_close(recompose_2d_affine(d), m);
    }

    #[test]
    fn decompose_pure_rotate_round_trip() {
        let theta = std::f32::consts::FRAC_PI_3; // 60°
        let m = [theta.cos(), theta.sin(), -theta.sin(), theta.cos(), 0.0, 0.0];
        let d = decompose_2d_affine(m);
        assert!(approx_eq(d.rotation, theta));
        assert!(approx_eq(d.scale_x, 1.0) && approx_eq(d.scale_y, 1.0));
        assert!(approx_eq(d.skew, 0.0));
        assert_affine_close(recompose_2d_affine(d), m);
    }

    #[test]
    fn decompose_pure_scale_round_trip() {
        let m = [2.0, 0.0, 0.0, 3.0, 0.0, 0.0];
        let d = decompose_2d_affine(m);
        assert!(approx_eq(d.scale_x, 2.0) && approx_eq(d.scale_y, 3.0));
        assert!(approx_eq(d.skew, 0.0) && approx_eq(d.rotation, 0.0));
        assert_affine_close(recompose_2d_affine(d), m);
    }

    #[test]
    fn decompose_pure_skew_x_round_trip() {
        // skewX(45°) → matrix(1, 0, tan(45), 1, 0, 0).
        let m = [1.0, 0.0, 1.0, 1.0, 0.0, 0.0];
        let d = decompose_2d_affine(m);
        assert!(approx_eq(d.skew, 1.0));
        assert!(approx_eq(d.scale_x, 1.0) && approx_eq(d.scale_y, 1.0));
        assert!(approx_eq(d.rotation, 0.0));
        assert_affine_close(recompose_2d_affine(d), m);
    }

    #[test]
    fn decompose_composite_round_trip() {
        // translate(10,20) * rotate(30°) * scale(2, 3).
        let theta = std::f32::consts::FRAC_PI_6;
        let cos = theta.cos();
        let sin = theta.sin();
        let m = [
            cos * 2.0,
            sin * 2.0,
            -sin * 3.0,
            cos * 3.0,
            10.0,
            20.0,
        ];
        let d = decompose_2d_affine(m);
        assert!(approx_eq(d.tx, 10.0) && approx_eq(d.ty, 20.0));
        assert!(approx_eq(d.rotation, theta));
        assert!(approx_eq(d.scale_x, 2.0) && approx_eq(d.scale_y, 3.0));
        assert!(approx_eq(d.skew, 0.0));
        assert_affine_close(recompose_2d_affine(d), m);
    }

    #[test]
    fn decompose_handles_reflection() {
        // scale(-1, 1) — отражение по X.
        let m = [-1.0, 0.0, 0.0, 1.0, 0.0, 0.0];
        let d = decompose_2d_affine(m);
        // det = -1 → scale_x должен быть отрицательным.
        assert!(d.scale_x < 0.0);
        assert_affine_close(recompose_2d_affine(d), m);
    }

    #[test]
    fn decompose_singular_matrix_yields_identity() {
        // Все нули — сингулярная матрица. Возвращаем identity-decomp,
        // чтобы recompose не паниковал на NaN-ах.
        let m = [0.0, 0.0, 0.0, 0.0, 5.0, 6.0];
        let d = decompose_2d_affine(m);
        assert_eq!(d, IDENTITY_DECOMP);
    }

    #[test]
    fn rotation_shortest_path_in_decompose() {
        // 170° → -170° через decompose должен пройти через 180°
        // (короткий путь 20°), не через 0° (длинный 340°).
        let pi = std::f32::consts::PI;
        let from = Decomposed2D {
            rotation: pi * 170.0 / 180.0,
            ..IDENTITY_DECOMP
        };
        let to = Decomposed2D {
            rotation: -pi * 170.0 / 180.0,
            ..IDENTITY_DECOMP
        };
        let mid = interpolate_decomposed(from, to, 0.5);
        // shortest-path: середина должна быть около ±180°, не около 0°.
        let abs_norm = mid.rotation.abs();
        assert!(
            (abs_norm - pi).abs() < 0.1,
            "expected ~π, got {} (=> {}°)",
            mid.rotation,
            mid.rotation.to_degrees()
        );
    }

    // ─── Affine composition consistency ───────────────────────────────────

    #[test]
    fn compose_chain_matches_property_trees_semantics() {
        // CSS: transform: translate(10,20) rotate(0°) — translate первое.
        let fns = vec![
            TransformFn::Translate(10.0, 20.0),
            TransformFn::Rotate(0.0),
        ];
        let m = compose_2d_affine(&fns);
        // Rotate(0) — identity, общий результат должен быть чистый translate.
        assert_affine_close(m, [1.0, 0.0, 0.0, 1.0, 10.0, 20.0]);
    }

    #[test]
    fn compose_translate_then_scale_position_of_origin() {
        // M = T(10,0) * S(2,2). Applied к (0,0) — даст (10, 0).
        let fns = vec![TransformFn::Translate(10.0, 0.0), TransformFn::Scale(2.0, 2.0)];
        let m = compose_2d_affine(&fns);
        // e = tx = 10, f = ty = 0, a = sx = 2, d = sy = 2.
        assert_affine_close(m, [2.0, 0.0, 0.0, 2.0, 10.0, 0.0]);
    }
}
