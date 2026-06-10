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

use crate::style::{
    AnimationDirection, AnimationFillMode, AnimationPlayState, Color, ComputedStyle, FilterFn,
    GradientStop, IterationCount, Length, TimingFunction, TransformFn,
};
use lumen_css_parser::{Declaration, KeyframesRule, Stylesheet};
use lumen_dom::NodeId;
use std::collections::HashMap;

// ─── P3 п.3A: scheduling types ──────────────────────────────────────────────

/// Sparse animated values for one element — scheduler output per node per frame.
/// P2 compositor reads `opacity` and `transform` to apply them without relayout.
#[derive(Debug, Clone, Default)]
pub struct AnimatedStyle {
    pub opacity: Option<f32>,
    pub transform: Option<Vec<TransformFn>>,
    pub color: Option<Color>,
    pub background_color: Option<Color>,
}

/// Output of `AnimationScheduler::tick` — per-node animated values for one frame.
/// `has_active` drives the `request_redraw` loop in the shell.
#[derive(Debug, Default)]
pub struct AnimationFrame {
    /// Node-level style overrides for this frame.
    pub overrides: HashMap<NodeId, AnimatedStyle>,
    /// True if at least one animation is still in its active period.
    pub has_active: bool,
}

impl AnimationFrame {
    /// Merge `other` into `self`; `other` values take precedence per property.
    ///
    /// Use to combine animation and transition frames in the shell frame loop:
    /// call `anim_frame.merge(transition_frame)` so transitions win on conflict.
    pub fn merge(&mut self, other: AnimationFrame) {
        self.has_active |= other.has_active;
        for (node, style) in other.overrides {
            let entry = self.overrides.entry(node).or_default();
            if let Some(v) = style.opacity { entry.opacity = Some(v); }
            if let Some(v) = style.transform { entry.transform = Some(v); }
            if let Some(v) = style.color { entry.color = Some(v); }
            if let Some(v) = style.background_color { entry.background_color = Some(v); }
        }
    }

    /// Extract only compositor-offloadable properties (opacity, transform).
    ///
    /// opacity and transform can be applied by patching the display list during
    /// paint without relayout. color/background-color require full relayout and
    /// stay in the caller's AnimationFrame for that path.
    /// Merge overrides from `other` into `self`. `other` values win on conflict.
    /// `has_active` becomes true if either frame has active animations.
    pub fn merge_from(&mut self, other: AnimationFrame) {
        self.has_active |= other.has_active;
        for (node, style) in other.overrides {
            let entry = self.overrides.entry(node).or_default();
            if style.opacity.is_some() { entry.opacity = style.opacity; }
            if style.transform.is_some() { entry.transform = style.transform; }
            if style.color.is_some() { entry.color = style.color; }
            if style.background_color.is_some() { entry.background_color = style.background_color; }
        }
    }

    /// Extract only compositor-offloadable properties (opacity, transform).
    ///
    /// opacity and transform can be applied by patching the display list during
    /// paint without relayout. color/background-color require full relayout and
    /// stay in the caller's AnimationFrame for that path.
    pub fn to_compositor_frame(&self) -> CompositorAnimFrame {
        let mut frame = CompositorAnimFrame {
            has_active: self.has_active,
            overrides: HashMap::new(),
        };
        for (&node, style) in &self.overrides {
            if style.opacity.is_some() || style.transform.is_some() {
                frame.overrides.insert(node, CompositorOverride {
                    opacity: style.opacity,
                    transform: style.transform.clone(),
                });
            }
        }
        frame
    }
}

/// Compositor-offloadable overrides for one element.
///
/// Only opacity and transform: these are applied as display-list patches
/// (PushOpacity / PushTransform) without relayout. color/background-color
/// require relayout and live in AnimatedStyle instead.
#[derive(Debug, Clone, Default)]
pub struct CompositorOverride {
    pub opacity: Option<f32>,
    pub transform: Option<Vec<TransformFn>>,
}

/// Per-frame compositor overrides — output of `AnimationFrame::to_compositor_frame`.
///
/// Passed to `build_display_list_with_anim` in lumen-paint to patch PushOpacity /
/// PushTransform commands per node without relayout.
#[derive(Debug, Default)]
pub struct CompositorAnimFrame {
    pub overrides: HashMap<NodeId, CompositorOverride>,
    pub has_active: bool,
}

impl CompositorAnimFrame {
    pub fn is_empty(&self) -> bool {
        self.overrides.is_empty()
    }

    pub fn get(&self, node: NodeId) -> Option<&CompositorOverride> {
        self.overrides.get(&node)
    }
}

/// Sparse style extracted from one `@keyframes` frame's declarations.
/// Only the commonly animated properties are populated.
#[derive(Debug, Clone, Default)]
pub struct KeyframeStyle {
    pub opacity: Option<f32>,
    pub transform: Option<Vec<TransformFn>>,
    pub color: Option<Color>,
    pub background_color: Option<Color>,
}

/// Parse the `declarations` of one `@keyframes` frame into a [`KeyframeStyle`].
/// Only CSS Animations commonly animated properties are extracted.
pub fn parse_keyframe_style(declarations: &[Declaration]) -> KeyframeStyle {
    let mut ks = KeyframeStyle::default();
    for decl in declarations {
        match decl.property.as_str() {
            "opacity" => {
                ks.opacity = decl.value.trim().parse::<f32>().ok().map(|v| v.clamp(0.0, 1.0));
            }
            "transform" => {
                ks.transform = Some(crate::style::parse_transform_list(decl.value.as_str()));
            }
            "color" => {
                ks.color = crate::style::parse_color(decl.value.as_str());
            }
            "background-color" => {
                ks.background_color = crate::style::parse_color(decl.value.as_str());
            }
            _ => {}
        }
    }
    ks
}

/// Анимируемое значение. Phase 0: восемь вариантов — Number / Length / Color /
/// TransformList / FilterList / GradientStops / Discrete (для non-interpolable
/// свойств).
///
/// Реальный список расширится дальше: Path-data для clip-path, shape-функции,
/// и т.д.
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
    /// CSS Filter Effects L1 §6 — список filter-функций. Пустой Vec
    /// соответствует `filter: none`. Интерполяция: matched-pair lerp
    /// при совпадающих типах и идентичных длинах; при несовпадающих
    /// длинах но prefix-match — недостающую сторону дополняем lacuna
    /// (identity) значениями; иначе — discrete (step-half).
    FilterList(Vec<FilterFn>),
    /// CSS Images L3 §3.5.1 — список `<color-stop>` градиента (без типа
    /// градиента — линейный/радиальный/конический держится у потребителя).
    /// Интерполяция: pairwise lerp цвета и позиции при идентичном числе
    /// stops и совместимых типах позиций. Любое несовпадение длины или
    /// unit-а позиции — `None` от helper-а → step-half у caller-а.
    GradientStops(Vec<GradientStop>),
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
            (AnimValue::FilterList(a), AnimValue::FilterList(b)) => Some(
                interpolate_filter_list(a, b, t)
                    .map(AnimValue::FilterList)
                    .unwrap_or_else(|| if t < 0.5 { from.clone() } else { to.clone() }),
            ),
            (AnimValue::GradientStops(a), AnimValue::GradientStops(b)) => Some(
                interpolate_gradient_stops(a, b, t)
                    .map(AnimValue::GradientStops)
                    .unwrap_or_else(|| if t < 0.5 { from.clone() } else { to.clone() }),
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

// ─── Filter-list interpolation (CSS Filter Effects L1 §6) ──────────────────

/// Интерполяция filter-list по CSS Filter Effects L1 §6. Возвращает
/// `Some(list)` при успешном matched-pair lerp; `None` сигнализирует
/// caller-у о необходимости discrete fallback (step-half).
///
/// Правила:
/// - Если обе стороны пусты — `none → none`, результат — пустой Vec.
/// - Если одна из сторон пустая, другая — непустая: трактуем пустую как
///   список lacuna (identity) значений, соответствующих позициям непустой
///   стороны. Это покрывает `filter: none ↔ filter: blur(10px)` спецификой
///   §6: «If only one filter is none, that side is treated as a list of
///   identity filter functions».
/// - При совпадающих длинах и совпадении `FilterFn` variant в каждой
///   позиции — per-position lerp числовых компонентов.
/// - При несовпадающих длинах, но prefix-match по типам — дополняем
///   короткую сторону lacuna-значениями типов из длинной стороны.
/// - Любое несовпадение типа в общем prefix → `None` (discrete).
pub(crate) fn interpolate_filter_list(
    from: &[FilterFn],
    to: &[FilterFn],
    t: f32,
) -> Option<Vec<FilterFn>> {
    if from.is_empty() && to.is_empty() {
        return Some(Vec::new());
    }

    // Проверяем prefix-match по типам на пересечении длин.
    let common = from.len().min(to.len());
    for i in 0..common {
        if !same_filter_kind(&from[i], &to[i]) {
            return None;
        }
    }

    let result_len = from.len().max(to.len());
    let mut result = Vec::with_capacity(result_len);
    for i in 0..result_len {
        let a_owned;
        let a: &FilterFn = if let Some(v) = from.get(i) {
            v
        } else {
            a_owned = filter_identity_for(&to[i]);
            &a_owned
        };
        let b_owned;
        let b: &FilterFn = if let Some(v) = to.get(i) {
            v
        } else {
            b_owned = filter_identity_for(&from[i]);
            &b_owned
        };
        result.push(interpolate_filter_fn_same_kind(a, b, t));
    }
    Some(result)
}

fn same_filter_kind(a: &FilterFn, b: &FilterFn) -> bool {
    use FilterFn::*;
    matches!(
        (a, b),
        (Blur(_), Blur(_))
            | (Brightness(_), Brightness(_))
            | (Contrast(_), Contrast(_))
            | (Grayscale(_), Grayscale(_))
            | (HueRotate(_), HueRotate(_))
            | (Invert(_), Invert(_))
            | (Opacity(_), Opacity(_))
            | (Saturate(_), Saturate(_))
            | (Sepia(_), Sepia(_))
    )
}

/// Lacuna (identity) value соответствующего типа — CSS Filter Effects L1 §6.
/// Identity такой, что применение фильтра эквивалентно отсутствию фильтра:
/// blur(0) / hue-rotate(0deg) / grayscale(0) / invert(0) / sepia(0) — нулевое
/// воздействие; brightness/contrast/opacity/saturate(1) — нейтральный множитель.
fn filter_identity_for(f: &FilterFn) -> FilterFn {
    match f {
        FilterFn::Blur(_) => FilterFn::Blur(0.0),
        FilterFn::Brightness(_) => FilterFn::Brightness(1.0),
        FilterFn::Contrast(_) => FilterFn::Contrast(1.0),
        FilterFn::Grayscale(_) => FilterFn::Grayscale(0.0),
        FilterFn::HueRotate(_) => FilterFn::HueRotate(0.0),
        FilterFn::Invert(_) => FilterFn::Invert(0.0),
        FilterFn::Opacity(_) => FilterFn::Opacity(1.0),
        FilterFn::Saturate(_) => FilterFn::Saturate(1.0),
        FilterFn::Sepia(_) => FilterFn::Sepia(0.0),
    }
}

/// Per-function lerp. Контракт: вызывается только когда `same_filter_kind`
/// истинно. Clamping значений в допустимый диапазон ([0,1] для
/// grayscale/invert/sepia) — задача consumer-а: spec позволяет
/// интерполировать «через» границу (например, brightness 0.5 → 2.0 даёт
/// промежуточные >1), а финальное применение фильтра трактует значения
/// согласно своему диапазону.
fn interpolate_filter_fn_same_kind(a: &FilterFn, b: &FilterFn, t: f32) -> FilterFn {
    use FilterFn::*;
    match (a, b) {
        (Blur(a), Blur(b)) => Blur(lerp_f32(*a, *b, t)),
        (Brightness(a), Brightness(b)) => Brightness(lerp_f32(*a, *b, t)),
        (Contrast(a), Contrast(b)) => Contrast(lerp_f32(*a, *b, t)),
        (Grayscale(a), Grayscale(b)) => Grayscale(lerp_f32(*a, *b, t)),
        (HueRotate(a), HueRotate(b)) => HueRotate(lerp_f32(*a, *b, t)),
        (Invert(a), Invert(b)) => Invert(lerp_f32(*a, *b, t)),
        (Opacity(a), Opacity(b)) => Opacity(lerp_f32(*a, *b, t)),
        (Saturate(a), Saturate(b)) => Saturate(lerp_f32(*a, *b, t)),
        (Sepia(a), Sepia(b)) => Sepia(lerp_f32(*a, *b, t)),
        _ => a.clone(),
    }
}

// ─── Gradient-stops interpolation (CSS Images L3 §3.5.1) ────────────────────

/// Интерполяция списка `<color-stop>` по CSS Images L3 §3.5.1
/// ("Interpolating Gradients").
///
/// Контракт:
/// - Оба списка пусты — `Some(empty)` (идемпотентная пара).
/// - Разная длина — `None`: spec требует поэлементного соответствия, иначе
///   discrete (`step-half` делает caller). Auto-распределение
///   `position: None` к used-value pre-interpolation — задача resolver-а;
///   на уровне `AnimValue` считаем, что входы уже сравнимы.
/// - Совпадение длин — pairwise lerp:
///   - `color` — линейно в sRGB (как у Color anywhere else в этом модуле).
///   - `position` — same-unit Length lerp; смешанные unit-ы или
///     `Some ↔ None` дают `None` от всей функции (consumer переключается
///     на step-half). Это консервативнее spec-а (он позволяет
///     pre-resolve auto stops), но не выдаёт визуально ломаных промежутков.
///   - `position` оба `None` — результат тоже `None` (auto → auto).
pub(crate) fn interpolate_gradient_stops(
    from: &[GradientStop],
    to: &[GradientStop],
    t: f32,
) -> Option<Vec<GradientStop>> {
    if from.is_empty() && to.is_empty() {
        return Some(Vec::new());
    }
    if from.len() != to.len() {
        return None;
    }

    let mut out = Vec::with_capacity(from.len());
    for (a, b) in from.iter().zip(to.iter()) {
        let color = interpolate_color(a.color, b.color, t);
        let position = match (&a.position, &b.position) {
            (None, None) => None,
            (Some(pa), Some(pb)) => Some(interpolate_length(pa, pb, t)?),
            _ => return None,
        };
        out.push(GradientStop { color, position });
    }
    Some(out)
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
        TransformFn::Rotate(theta) | TransformFn::RotateZ(theta) => {
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
        // 3D functions have no 2D affine equivalent — use identity for
        // the legacy 2D animation path; the full Mat4 path handles them.
        _ => IDENTITY,
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

// ─── AnimationScheduler ─────────────────────────────────────────────────────

/// CSS Animations L1 §3 — scheduler that maps `@keyframes` to interpolated
/// `AnimatedStyle` values for each active animation on each frame tick.
///
/// Lifecycle:
/// 1. Call [`AnimationScheduler::sync`] after relayout to register/update
///    which animations are active for a node.
/// 2. Call [`AnimationScheduler::tick`] each frame to get `AnimationFrame`
///    that P2 compositor applies (opacity / transform without relayout).
#[derive(Debug, Default)]
pub struct AnimationScheduler {
    /// (NodeId, animation-list-index) → wall-clock start time in seconds.
    start_times: HashMap<(NodeId, usize), f32>,
}

impl AnimationScheduler {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register or refresh animations for `node` based on its computed style.
    ///
    /// New animations receive `now` as their start time. Existing animations
    /// keep their original start time (so they don't restart on unrelated
    /// relayouts). Animations whose name is `"none"` or that are absent from
    /// the new style are removed.
    pub fn sync(&mut self, node: NodeId, style: &ComputedStyle, now: f32) {
        // Remove stale entries for this node.
        self.start_times.retain(|(n, idx), _| {
            if *n != node {
                return true;
            }
            style
                .animation_names
                .get(*idx)
                .is_some_and(|name| !name.is_empty() && name != "none")
        });
        // Register newly appearing animations.
        for (idx, name) in style.animation_names.iter().enumerate() {
            if name.is_empty() || name == "none" {
                continue;
            }
            self.start_times.entry((node, idx)).or_insert(now);
        }
    }

    /// Remove all animation state for `node` (e.g. when the node is removed from the DOM).
    pub fn remove_node(&mut self, node: NodeId) {
        self.start_times.retain(|(n, _), _| *n != node);
    }

    /// Compute per-node animated style overrides for the current frame.
    ///
    /// `style_getter` returns the `ComputedStyle` for a `NodeId` so the
    /// scheduler can read `animation_*` properties. `sheet` is the current
    /// stylesheet (provides `@keyframes` rules). `now` is wall-clock time in
    /// seconds.
    pub fn tick(
        &self,
        sheet: &Stylesheet,
        style_getter: impl Fn(NodeId) -> Option<ComputedStyle>,
        now: f32,
    ) -> AnimationFrame {
        let mut frame = AnimationFrame::default();

        for (&(node, anim_idx), &start_time) in &self.start_times {
            let Some(style) = style_getter(node) else {
                continue;
            };
            let Some(ks) =
                compute_animation_value(anim_idx, &style, sheet, start_time, now)
            else {
                continue;
            };
            let entry = frame.overrides.entry(node).or_default();
            if let Some(op) = ks.opacity {
                entry.opacity = Some(op);
            }
            if let Some(tr) = ks.transform {
                entry.transform = Some(tr);
            }
            if let Some(c) = ks.color {
                entry.color = Some(c);
            }
            if let Some(bg) = ks.background_color {
                entry.background_color = Some(bg);
            }
            frame.has_active = true;
        }

        frame
    }
}

/// Compute the interpolated `KeyframeStyle` for a single animation at `now`.
/// Returns `None` when the animation is not in its active period (finished and
/// no fill-mode, or paused, or duration=0).
fn compute_animation_value(
    anim_idx: usize,
    style: &ComputedStyle,
    sheet: &Stylesheet,
    start_time: f32,
    now: f32,
) -> Option<KeyframeStyle> {
    let name = cyclic_get(&style.animation_names, anim_idx)?;
    if name.is_empty() || name == "none" {
        return None;
    }

    let duration = cyclic_get(&style.animation_durations, anim_idx)
        .copied()
        .unwrap_or(0.0);
    if duration <= 0.0 {
        return None;
    }

    let delay = cyclic_get(&style.animation_delays, anim_idx)
        .copied()
        .unwrap_or(0.0);
    let timing_fn = cyclic_get(&style.animation_timing_functions, anim_idx)
        .cloned()
        .unwrap_or_else(TimingFunction::default);
    let iter_count = cyclic_get(&style.animation_iteration_counts, anim_idx)
        .cloned()
        .unwrap_or_default();
    let direction = cyclic_get(&style.animation_directions, anim_idx)
        .cloned()
        .unwrap_or_default();
    let fill_mode = cyclic_get(&style.animation_fill_modes, anim_idx)
        .cloned()
        .unwrap_or_default();
    let play_state = cyclic_get(&style.animation_play_states, anim_idx)
        .cloned()
        .unwrap_or_default();

    if matches!(play_state, AnimationPlayState::Paused) {
        return None;
    }

    let kf = sheet.keyframes.iter().find(|k| k.name == *name)?;

    let elapsed = now - start_time - delay;

    if elapsed < 0.0 {
        // Still in delay: apply first keyframe if fill-mode includes backwards.
        if matches!(
            fill_mode,
            AnimationFillMode::Backwards | AnimationFillMode::Both
        ) {
            return keyframe_at(kf, 0.0, &timing_fn, &direction);
        }
        return None;
    }

    let max_iters = match iter_count {
        IterationCount::Infinite => f32::INFINITY,
        IterationCount::Finite(n) => n,
    };
    let total = duration * max_iters;

    if elapsed >= total {
        // Animation has ended: apply last keyframe if fill-mode includes forwards.
        if matches!(
            fill_mode,
            AnimationFillMode::Forwards | AnimationFillMode::Both
        ) {
            return keyframe_at(kf, 1.0, &timing_fn, &direction);
        }
        return None;
    }

    // Current iteration progress [0, 1].
    let iter_floor = (elapsed / duration).floor();
    let local = elapsed - iter_floor * duration;
    let raw_t = (local / duration).clamp(0.0, 1.0);

    // Apply animation-direction (CSS Animations L1 §3.6).
    let is_odd = (iter_floor as u64) % 2 == 1;
    let directed_t = match direction {
        AnimationDirection::Normal => raw_t,
        AnimationDirection::Reverse => 1.0 - raw_t,
        AnimationDirection::Alternate => {
            if is_odd { 1.0 - raw_t } else { raw_t }
        }
        AnimationDirection::AlternateReverse => {
            if is_odd { raw_t } else { 1.0 - raw_t }
        }
    };

    let eased_t = timing_fn.progress(directed_t);
    keyframe_interpolate(kf, eased_t)
}

/// Returns the `KeyframeStyle` at overall animation progress `t ∈ [0, 1]`,
/// after applying `timing_fn` and `direction` to the global t.
/// (Used for fill-mode endpoints where we want the rendered value at the
/// boundary with timing and direction already baked in.)
fn keyframe_at(
    kf: &KeyframesRule,
    t: f32,
    timing_fn: &TimingFunction,
    direction: &AnimationDirection,
) -> Option<KeyframeStyle> {
    // For fill-mode boundaries we keep t = 0.0 or 1.0 without easing, as per
    // CSS Animations L1 §4.5: "The initial value (0%) keyframe at fill backwards,
    // final value (100%) at fill forwards."
    let _ = (timing_fn, direction);
    keyframe_interpolate(kf, t)
}

/// Find the two surrounding `@keyframes` stops for progress `t ∈ [0, 1]` and
/// interpolate them using [`LinearInterpolator`].
fn keyframe_interpolate(kf: &KeyframesRule, t: f32) -> Option<KeyframeStyle> {
    if kf.frames.is_empty() {
        return None;
    }

    // Sort by offset ascending.
    let mut sorted: Vec<&lumen_css_parser::Keyframe> = kf.frames.iter().collect();
    sorted.sort_by(|a, b| a.offset.partial_cmp(&b.offset).unwrap_or(std::cmp::Ordering::Equal));

    // Surrounding frames.
    let from_frame = sorted.iter().rev().find(|f| f.offset <= t).copied()
        .unwrap_or(sorted[0]);
    let to_frame = sorted.iter().find(|f| f.offset >= t).copied()
        .unwrap_or_else(|| sorted[sorted.len() - 1]);

    let from_ks = parse_keyframe_style(&from_frame.declarations);
    let to_ks = parse_keyframe_style(&to_frame.declarations);

    // Local t between the two surrounding stops.
    let span = to_frame.offset - from_frame.offset;
    let local_t = if span > f32::EPSILON {
        ((t - from_frame.offset) / span).clamp(0.0, 1.0)
    } else {
        1.0
    };

    let interp = LinearInterpolator;
    Some(KeyframeStyle {
        opacity: interp_optional_f32(from_ks.opacity, to_ks.opacity, local_t, &interp),
        transform: interp_optional_transform(
            from_ks.transform.as_deref(),
            to_ks.transform.as_deref(),
            local_t,
            &interp,
        ),
        color: interp_optional_color(from_ks.color, to_ks.color, local_t, &interp),
        background_color: interp_optional_color(
            from_ks.background_color,
            to_ks.background_color,
            local_t,
            &interp,
        ),
    })
}

fn interp_optional_f32(
    from: Option<f32>,
    to: Option<f32>,
    t: f32,
    interp: &impl AnimationInterpolator,
) -> Option<f32> {
    match (from, to) {
        (Some(f), Some(t_val)) => interp
            .interpolate(&AnimValue::Number(f), &AnimValue::Number(t_val), t)
            .and_then(|v| if let AnimValue::Number(n) = v { Some(n) } else { None }),
        (Some(f), None) => Some(f),
        (None, Some(t_val)) => Some(t_val),
        (None, None) => None,
    }
}

fn interp_optional_color(
    from: Option<Color>,
    to: Option<Color>,
    t: f32,
    interp: &impl AnimationInterpolator,
) -> Option<Color> {
    match (from, to) {
        (Some(f), Some(t_val)) => interp
            .interpolate(&AnimValue::Color(f), &AnimValue::Color(t_val), t)
            .and_then(|v| if let AnimValue::Color(c) = v { Some(c) } else { None }),
        (Some(f), None) => Some(f),
        (None, Some(t_val)) => Some(t_val),
        (None, None) => None,
    }
}

fn interp_optional_transform(
    from: Option<&[TransformFn]>,
    to: Option<&[TransformFn]>,
    t: f32,
    interp: &impl AnimationInterpolator,
) -> Option<Vec<TransformFn>> {
    match (from, to) {
        (Some(f), Some(t_val)) => interp
            .interpolate(
                &AnimValue::TransformList(f.to_vec()),
                &AnimValue::TransformList(t_val.to_vec()),
                t,
            )
            .and_then(|v| {
                if let AnimValue::TransformList(tr) = v {
                    Some(tr)
                } else {
                    None
                }
            }),
        (Some(f), None) => Some(f.to_vec()),
        (None, Some(t_val)) => Some(t_val.to_vec()),
        (None, None) => None,
    }
}

/// Cyclically access list element at `idx`, reusing if `idx >= list.len()`.
/// Returns `None` only when list is empty.
fn cyclic_get<T>(list: &[T], idx: usize) -> Option<&T> {
    if list.is_empty() {
        None
    } else {
        list.get(idx % list.len())
    }
}

// ─── CSS Transitions L1 §2 — TransitionScheduler ────────────────────────────

/// State for one active property transition on one element.
///
/// Supports CSS Transitions L2 features: fill-mode and interrupted transitions.
#[derive(Debug, Clone)]
struct TransitionState {
    from: AnimValue,
    to: AnimValue,
    start_time: f32,
    duration: f32,
    delay: f32,
    timing_fn: TimingFunction,
    /// CSS Transitions L2: animation-fill-mode for transitions.
    /// Determines values before delay and after completion.
    fill_mode: AnimationFillMode,
    /// Interrupted transition: stores the value at interruption point.
    /// When a new transition starts while the previous one is active,
    /// this preserves the interrupted value for the new `from` calculation.
    #[allow(dead_code)]
    interrupted_value: Option<AnimValue>,
}

/// CSS Transitions L1 §2 — detects property value changes and interpolates
/// them over the transition duration.
///
/// Unlike `AnimationScheduler` (which uses `@keyframes` and runs on a timer),
/// transitions are *reactive*: they start when a computed property value
/// changes. Call `sync()` after each relayout that may change computed styles.
///
/// Phase 0 animatable properties: `opacity`, `color`, `background-color`,
/// `transform`. `transition-property: all` checks all four.
#[derive(Debug, Default)]
pub struct TransitionScheduler {
    /// Active transitions keyed by `(node, css-property-name)`.
    active: HashMap<(NodeId, String), TransitionState>,
}

impl TransitionScheduler {
    pub fn new() -> Self {
        Self::default()
    }

    /// Detect value changes between `old` and `new` style for properties listed
    /// in `new.transition_properties` and start (or update) transitions.
    ///
    /// # CSS: @starting-style
    /// When `node` has just entered the document (check `StartingStyleTracker::is_entered`),
    /// substitute `old` with a style built from `resolve_starting_style(node, doc, sheet)`
    /// instead of the node's prior computed style. After substitution call
    /// `tracker.consume(node)`. This enables enter-animations per CSS Transitions L2 §3.4.
    /// See `crate::starting_style::{StartingStyleTracker, resolve_starting_style}`.
    pub fn sync(&mut self, node: NodeId, old: &ComputedStyle, new: &ComputedStyle, now: f32) {
        if new.transition_properties.is_empty() {
            return;
        }
        let check_all = new
            .transition_properties
            .iter()
            .any(|p| p.eq_ignore_ascii_case("all"));

        type PropExtractor = (&'static str, fn(&ComputedStyle) -> AnimValue);
        // Table of Phase-0 animatable properties and how to extract AnimValue.
        let animatable: [PropExtractor; 4] = [
            ("opacity", |s| AnimValue::Number(s.opacity)),
            ("color", |s| AnimValue::Color(s.color)),
            ("background-color", |s| {
                AnimValue::Color(
                    s.background_color
                        .map_or(Color::TRANSPARENT, |c| c.resolve(s.color)),
                )
            }),
            ("transform", |s| AnimValue::TransformList(s.transform.clone())),
        ];

        for (prop_idx, (prop_name, extract)) in animatable.iter().enumerate() {
            let is_listed = check_all
                || new
                    .transition_properties
                    .iter()
                    .any(|p| p.eq_ignore_ascii_case(prop_name));
            if !is_listed {
                continue;
            }

            let dur = cyclic_get(&new.transition_durations, prop_idx)
                .copied()
                .unwrap_or(0.0);
            if dur <= 0.0 {
                self.active.remove(&(node, prop_name.to_string()));
                continue;
            }

            let to_val = extract(new);

            // Check if there's an active transition that will be interrupted.
            // If interrupted, use the previous `to` value as the starting point for smooth continuation.
            let interrupted_value = self
                .active
                .get(&(node, prop_name.to_string()))
                .map(|state| state.to.clone());

            let from_val = interrupted_value.clone().unwrap_or_else(|| extract(old));

            if from_val == to_val {
                continue;
            }

            let delay = cyclic_get(&new.transition_delays, prop_idx)
                .copied()
                .unwrap_or(0.0);
            let timing_fn = cyclic_get(&new.transition_timing_functions, prop_idx)
                .cloned()
                .unwrap_or_else(TimingFunction::default);
            let fill_mode = cyclic_get(&new.transition_fill_modes, prop_idx)
                .copied()
                .unwrap_or(AnimationFillMode::None);

            self.active.insert(
                (node, prop_name.to_string()),
                TransitionState {
                    from: from_val,
                    to: to_val,
                    start_time: now,
                    duration: dur,
                    delay,
                    timing_fn,
                    fill_mode,
                    interrupted_value,
                },
            );
        }
    }

    /// Remove all transition state for `node` (called when node leaves DOM).
    pub fn remove_node(&mut self, node: NodeId) {
        self.active.retain(|(n, _), _| *n != node);
    }

    /// Apply a transition value to the animated style entry.
    fn apply_transition_value_to_entry(val: &AnimValue, prop: &str, entry: &mut AnimatedStyle) {
        match prop {
            "opacity" => {
                if let AnimValue::Number(n) = val {
                    entry.opacity = Some(*n);
                }
            }
            "color" => {
                if let AnimValue::Color(c) = val {
                    entry.color = Some(*c);
                }
            }
            "background-color" => {
                if let AnimValue::Color(c) = val {
                    entry.background_color = Some(*c);
                }
            }
            "transform" => {
                if let AnimValue::TransformList(tr) = val {
                    entry.transform = Some(tr.clone());
                }
            }
            _ => {}
        }
    }

    /// Compute interpolated style overrides for the current frame.
    /// Completed transitions are removed unless fill_mode preserves them.
    pub fn tick(&mut self, now: f32) -> AnimationFrame {
        let mut frame = AnimationFrame::default();
        let interp = LinearInterpolator;

        self.active.retain(|(node, prop), state| {
            let elapsed = now - state.start_time - state.delay;
            if elapsed < 0.0 {
                // Still in delay period.
                // Apply fill-mode backwards if enabled.
                if matches!(
                    state.fill_mode,
                    AnimationFillMode::Backwards | AnimationFillMode::Both
                ) && let Some(val) = interp.interpolate(&state.from, &state.to, 0.0) {
                    let entry = frame.overrides.entry(*node).or_default();
                    Self::apply_transition_value_to_entry(&val, prop, entry);
                }
                frame.has_active = true;
                return true;
            }
            if elapsed >= state.duration {
                // Transition complete.
                // Apply fill-mode forwards if enabled.
                if matches!(
                    state.fill_mode,
                    AnimationFillMode::Forwards | AnimationFillMode::Both
                ) {
                    if let Some(val) = interp.interpolate(&state.from, &state.to, 1.0) {
                        let entry = frame.overrides.entry(*node).or_default();
                        Self::apply_transition_value_to_entry(&val, prop, entry);
                    }
                    frame.has_active = true;
                    return true;
                }
                // Transition complete — remove.
                return false;
            }

            let raw_t = (elapsed / state.duration).clamp(0.0, 1.0);
            let eased_t = state.timing_fn.progress(raw_t);

            if let Some(val) = interp.interpolate(&state.from, &state.to, eased_t) {
                let entry = frame.overrides.entry(*node).or_default();
                Self::apply_transition_value_to_entry(&val, prop, entry);
            }
            frame.has_active = true;
            true
        });

        frame
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

    // ─── FilterList interpolation (CSS Filter Effects L1 §6) ──────────────

    fn close_filter(a: &FilterFn, b: &FilterFn) -> bool {
        use FilterFn::*;
        match (a, b) {
            (Blur(x), Blur(y))
            | (Brightness(x), Brightness(y))
            | (Contrast(x), Contrast(y))
            | (Grayscale(x), Grayscale(y))
            | (HueRotate(x), HueRotate(y))
            | (Invert(x), Invert(y))
            | (Opacity(x), Opacity(y))
            | (Saturate(x), Saturate(y))
            | (Sepia(x), Sepia(y)) => (x - y).abs() < 1e-5,
            _ => false,
        }
    }

    fn assert_filter_list_close(actual: &[FilterFn], expected: &[FilterFn]) {
        assert_eq!(
            actual.len(),
            expected.len(),
            "len mismatch: actual {actual:?}, expected {expected:?}"
        );
        for (i, (a, e)) in actual.iter().zip(expected.iter()).enumerate() {
            assert!(
                close_filter(a, e),
                "filter[{i}] mismatch: actual {a:?}, expected {e:?}"
            );
        }
    }

    #[test]
    fn filter_list_none_to_none_stays_empty() {
        let r = interpolate_filter_list(&[], &[], 0.5);
        assert_eq!(r.as_deref(), Some(&[] as &[FilterFn]));
    }

    #[test]
    fn filter_list_same_kind_single_lerps_value() {
        // blur(0) → blur(10px) at t=0.5 → blur(5)
        let from = [FilterFn::Blur(0.0)];
        let to = [FilterFn::Blur(10.0)];
        let r = interpolate_filter_list(&from, &to, 0.5).expect("matched-pair");
        assert_filter_list_close(&r, &[FilterFn::Blur(5.0)]);
    }

    #[test]
    fn filter_list_multi_matched_pair_lerps_each() {
        // [blur(0), brightness(1)] → [blur(8), brightness(2)] at t=0.25
        let from = [FilterFn::Blur(0.0), FilterFn::Brightness(1.0)];
        let to = [FilterFn::Blur(8.0), FilterFn::Brightness(2.0)];
        let r = interpolate_filter_list(&from, &to, 0.25).expect("matched-pair");
        assert_filter_list_close(&r, &[FilterFn::Blur(2.0), FilterFn::Brightness(1.25)]);
    }

    #[test]
    fn filter_list_endpoints_return_exact() {
        let from = [FilterFn::Grayscale(0.0), FilterFn::Sepia(0.0)];
        let to = [FilterFn::Grayscale(1.0), FilterFn::Sepia(1.0)];
        let r0 = interpolate_filter_list(&from, &to, 0.0).expect("matched-pair");
        let r1 = interpolate_filter_list(&from, &to, 1.0).expect("matched-pair");
        assert_filter_list_close(&r0, &from);
        assert_filter_list_close(&r1, &to);
    }

    #[test]
    fn filter_list_kind_mismatch_returns_none() {
        // blur(...) vs brightness(...) на одной позиции → discrete fallback.
        let from = [FilterFn::Blur(5.0)];
        let to = [FilterFn::Brightness(2.0)];
        assert_eq!(interpolate_filter_list(&from, &to, 0.5), None);
    }

    #[test]
    fn filter_list_prefix_mismatch_anywhere_returns_none() {
        // Префикс blur=blur матчится, второй элемент grayscale ≠ contrast →
        // вся пара уходит в discrete.
        let from = [FilterFn::Blur(0.0), FilterFn::Grayscale(0.0)];
        let to = [FilterFn::Blur(10.0), FilterFn::Contrast(2.0)];
        assert_eq!(interpolate_filter_list(&from, &to, 0.5), None);
    }

    #[test]
    fn filter_list_none_to_single_pads_with_identity() {
        // [] → [blur(10)]: пустая сторона трактуется как [blur(0)].
        // t=0.5 → [blur(5)].
        let r = interpolate_filter_list(&[], &[FilterFn::Blur(10.0)], 0.5)
            .expect("identity-padded");
        assert_filter_list_close(&r, &[FilterFn::Blur(5.0)]);
    }

    #[test]
    fn filter_list_single_to_none_pads_with_identity() {
        // [brightness(0.5)] → []: пустая правая → [brightness(1)] identity.
        // t=0.5 → brightness(0.75).
        let r = interpolate_filter_list(&[FilterFn::Brightness(0.5)], &[], 0.5)
            .expect("identity-padded");
        assert_filter_list_close(&r, &[FilterFn::Brightness(0.75)]);
    }

    #[test]
    fn filter_list_shorter_prefix_padded_in_long_side() {
        // [blur(0)] → [blur(10), grayscale(1)]:
        // позиция 0 matched (blur), позиция 1 — left is None,
        // дополняем left grayscale(0) (identity).
        // t=0.5 → [blur(5), grayscale(0.5)].
        let from = [FilterFn::Blur(0.0)];
        let to = [FilterFn::Blur(10.0), FilterFn::Grayscale(1.0)];
        let r = interpolate_filter_list(&from, &to, 0.5).expect("padded prefix-match");
        assert_filter_list_close(&r, &[FilterFn::Blur(5.0), FilterFn::Grayscale(0.5)]);
    }

    #[test]
    fn filter_list_long_to_short_pads_right() {
        // [contrast(2), sepia(1)] → [contrast(0.5)]:
        // позиция 0 matched, позиция 1 — right is None, padding sepia(0).
        // t=0.25 → contrast(1.625), sepia(0.75).
        let from = [FilterFn::Contrast(2.0), FilterFn::Sepia(1.0)];
        let to = [FilterFn::Contrast(0.5)];
        let r = interpolate_filter_list(&from, &to, 0.25).expect("padded prefix-match");
        assert_filter_list_close(
            &r,
            &[FilterFn::Contrast(1.625), FilterFn::Sepia(0.75)],
        );
    }

    #[test]
    fn filter_list_hue_rotate_lerps_radians() {
        // Парсер сохраняет hue-rotate в радианах. Линейный lerp без
        // shortest-path: 0 → π at t=0.5 = π/2.
        let pi = std::f32::consts::PI;
        let r = interpolate_filter_list(
            &[FilterFn::HueRotate(0.0)],
            &[FilterFn::HueRotate(pi)],
            0.5,
        )
        .expect("matched-pair");
        assert_filter_list_close(&r, &[FilterFn::HueRotate(pi / 2.0)]);
    }

    // ─── LinearInterpolator wraps FilterList correctly ────────────────────

    #[test]
    fn linear_interpolator_filter_matched_pair() {
        let interp = LinearInterpolator;
        let from = AnimValue::FilterList(vec![FilterFn::Blur(0.0)]);
        let to = AnimValue::FilterList(vec![FilterFn::Blur(10.0)]);
        let r = interp.interpolate(&from, &to, 0.5).expect("some");
        match r {
            AnimValue::FilterList(list) => {
                assert_filter_list_close(&list, &[FilterFn::Blur(5.0)])
            }
            other => panic!("expected FilterList, got {other:?}"),
        }
    }

    #[test]
    fn linear_interpolator_filter_discrete_on_kind_mismatch() {
        // Mismatched kinds → step-half через клонирование from/to.
        let interp = LinearInterpolator;
        let from = AnimValue::FilterList(vec![FilterFn::Blur(5.0)]);
        let to = AnimValue::FilterList(vec![FilterFn::Brightness(2.0)]);
        assert_eq!(interp.interpolate(&from, &to, 0.3), Some(from.clone()));
        assert_eq!(interp.interpolate(&from, &to, 0.7), Some(to.clone()));
    }

    #[test]
    fn linear_interpolator_filter_none_to_filter_pads() {
        // FilterList([]) → FilterList([brightness(2)]) at t=0.5 → [brightness(1.5)].
        let interp = LinearInterpolator;
        let from = AnimValue::FilterList(Vec::new());
        let to = AnimValue::FilterList(vec![FilterFn::Brightness(2.0)]);
        match interp.interpolate(&from, &to, 0.5).expect("some") {
            AnimValue::FilterList(list) => {
                assert_filter_list_close(&list, &[FilterFn::Brightness(1.5)])
            }
            other => panic!("expected FilterList, got {other:?}"),
        }
    }

    #[test]
    fn linear_interpolator_filter_endpoints_exact() {
        let interp = LinearInterpolator;
        let from = AnimValue::FilterList(vec![FilterFn::Invert(0.0)]);
        let to = AnimValue::FilterList(vec![FilterFn::Invert(1.0)]);
        assert_eq!(interp.interpolate(&from, &to, 0.0), Some(from.clone()));
        assert_eq!(interp.interpolate(&from, &to, 1.0), Some(to.clone()));
    }

    // ─── Gradient-stops interpolation ───────────────────────────────────────

    fn stop(r: u8, g: u8, b: u8, pos: Option<Length>) -> GradientStop {
        GradientStop {
            color: Color { r, g, b, a: 255 },
            position: pos,
        }
    }

    fn assert_color_close(a: Color, expected: Color) {
        assert!(
            a.r.abs_diff(expected.r) <= 1
                && a.g.abs_diff(expected.g) <= 1
                && a.b.abs_diff(expected.b) <= 1
                && a.a.abs_diff(expected.a) <= 1,
            "color mismatch: got {a:?}, expected {expected:?}"
        );
    }

    #[test]
    fn gradient_stops_empty_pair_returns_empty() {
        assert_eq!(interpolate_gradient_stops(&[], &[], 0.5), Some(Vec::new()));
    }

    #[test]
    fn gradient_stops_mismatched_length_is_none() {
        let a = vec![stop(255, 0, 0, Some(Length::Percent(0.0)))];
        let b = vec![
            stop(255, 0, 0, Some(Length::Percent(0.0))),
            stop(0, 0, 255, Some(Length::Percent(100.0))),
        ];
        assert!(interpolate_gradient_stops(&a, &b, 0.5).is_none());
    }

    #[test]
    fn gradient_stops_matched_pair_lerps_color_and_percent() {
        // red @0% → blue @100%  at t=0.5  →  (128,0,128) @50%.
        let from = vec![
            stop(255, 0, 0, Some(Length::Percent(0.0))),
            stop(0, 0, 255, Some(Length::Percent(100.0))),
        ];
        let to = vec![
            stop(0, 0, 255, Some(Length::Percent(0.0))),
            stop(255, 0, 0, Some(Length::Percent(100.0))),
        ];
        let res = interpolate_gradient_stops(&from, &to, 0.5).expect("some");
        assert_eq!(res.len(), 2);
        assert_color_close(res[0].color, Color { r: 128, g: 0, b: 128, a: 255 });
        assert_color_close(res[1].color, Color { r: 128, g: 0, b: 128, a: 255 });
        // Percent → Percent на одинаковых endpoints (0%, 100%) — без сдвига.
        assert_eq!(res[0].position, Some(Length::Percent(0.0)));
        assert_eq!(res[1].position, Some(Length::Percent(100.0)));
    }

    #[test]
    fn gradient_stops_lerps_position_within_same_unit() {
        // Сдвигаем второй stop с 100% к 50% — на t=0.5 ждём 75%.
        let from = vec![
            stop(255, 0, 0, Some(Length::Percent(0.0))),
            stop(0, 0, 255, Some(Length::Percent(100.0))),
        ];
        let to = vec![
            stop(255, 0, 0, Some(Length::Percent(0.0))),
            stop(0, 0, 255, Some(Length::Percent(50.0))),
        ];
        let res = interpolate_gradient_stops(&from, &to, 0.5).expect("some");
        assert_eq!(res[1].position, Some(Length::Percent(75.0)));
    }

    #[test]
    fn gradient_stops_mixed_units_return_none() {
        // px ↔ % несовместимы без resolve в used px → step-half у caller-а.
        let from = vec![stop(255, 0, 0, Some(Length::Px(10.0)))];
        let to = vec![stop(0, 0, 255, Some(Length::Percent(50.0)))];
        assert!(interpolate_gradient_stops(&from, &to, 0.5).is_none());
    }

    #[test]
    fn gradient_stops_some_to_none_is_none() {
        // Один stop с фиксированной позицией, второй — auto-распределение:
        // pair несовместима без pre-resolve, итог — discrete.
        let from = vec![stop(255, 0, 0, Some(Length::Percent(0.0)))];
        let to = vec![stop(0, 0, 255, None)];
        assert!(interpolate_gradient_stops(&from, &to, 0.5).is_none());
    }

    #[test]
    fn gradient_stops_both_none_positions_preserved() {
        // auto → auto — позиция остаётся None, цвет lerp-ится.
        let from = vec![stop(0, 0, 0, None)];
        let to = vec![stop(255, 255, 255, None)];
        let res = interpolate_gradient_stops(&from, &to, 0.5).expect("some");
        assert_eq!(res[0].position, None);
        assert_color_close(res[0].color, Color { r: 128, g: 128, b: 128, a: 255 });
    }

    #[test]
    fn gradient_stops_endpoints_t_zero_and_one() {
        // Pixel-positions lerp на endpoints: t=0 ≈ from, t=1 ≈ to.
        let from = vec![
            stop(10, 20, 30, Some(Length::Px(0.0))),
            stop(40, 50, 60, Some(Length::Px(100.0))),
        ];
        let to = vec![
            stop(70, 80, 90, Some(Length::Px(0.0))),
            stop(100, 110, 120, Some(Length::Px(200.0))),
        ];
        let at_zero = interpolate_gradient_stops(&from, &to, 0.0).expect("some");
        assert_color_close(at_zero[0].color, from[0].color);
        assert_color_close(at_zero[1].color, from[1].color);
        assert_eq!(at_zero[1].position, Some(Length::Px(100.0)));

        let at_one = interpolate_gradient_stops(&from, &to, 1.0).expect("some");
        assert_color_close(at_one[0].color, to[0].color);
        assert_color_close(at_one[1].color, to[1].color);
        assert_eq!(at_one[1].position, Some(Length::Px(200.0)));
    }

    #[test]
    fn linear_interpolator_gradient_stops_smooth() {
        let interp = LinearInterpolator;
        let from = AnimValue::GradientStops(vec![
            stop(255, 0, 0, Some(Length::Percent(0.0))),
            stop(0, 0, 255, Some(Length::Percent(100.0))),
        ]);
        let to = AnimValue::GradientStops(vec![
            stop(0, 0, 255, Some(Length::Percent(0.0))),
            stop(255, 0, 0, Some(Length::Percent(100.0))),
        ]);
        match interp.interpolate(&from, &to, 0.5).expect("some") {
            AnimValue::GradientStops(stops) => {
                assert_eq!(stops.len(), 2);
                assert_color_close(stops[0].color, Color { r: 128, g: 0, b: 128, a: 255 });
                assert_color_close(stops[1].color, Color { r: 128, g: 0, b: 128, a: 255 });
            }
            other => panic!("expected GradientStops, got {other:?}"),
        }
    }

    #[test]
    fn linear_interpolator_gradient_stops_discrete_on_length_mismatch() {
        // Разная длина → caller получает step-half клон from/to.
        let interp = LinearInterpolator;
        let from = AnimValue::GradientStops(vec![stop(255, 0, 0, Some(Length::Percent(0.0)))]);
        let to = AnimValue::GradientStops(vec![
            stop(0, 0, 255, Some(Length::Percent(0.0))),
            stop(0, 255, 0, Some(Length::Percent(100.0))),
        ]);
        assert_eq!(interp.interpolate(&from, &to, 0.3), Some(from.clone()));
        assert_eq!(interp.interpolate(&from, &to, 0.7), Some(to.clone()));
    }

    #[test]
    fn linear_interpolator_gradient_stops_endpoints_exact() {
        let interp = LinearInterpolator;
        let from = AnimValue::GradientStops(vec![stop(255, 0, 0, Some(Length::Percent(0.0)))]);
        let to = AnimValue::GradientStops(vec![stop(0, 0, 255, Some(Length::Percent(100.0)))]);
        assert_eq!(interp.interpolate(&from, &to, 0.0), Some(from.clone()));
        assert_eq!(interp.interpolate(&from, &to, 1.0), Some(to.clone()));
    }

    // ─────── AnimationScheduler ───────

    fn make_style_with_anim(name: &str, duration: f32) -> ComputedStyle {
        let mut s = ComputedStyle::root();
        s.animation_names = vec![name.to_string()];
        s.animation_durations = vec![duration];
        s.animation_timing_functions = vec![crate::style::TimingFunction::Linear];
        s
    }

    fn make_sheet_opacity(name: &str) -> lumen_css_parser::Stylesheet {
        let css = format!(
            "@keyframes {name} {{ \
                from {{ opacity: 0; }} \
                to   {{ opacity: 1; }} \
            }}"
        );
        lumen_css_parser::parse(&css)
    }

    #[test]
    fn scheduler_sync_registers_animation() {
        let mut sched = AnimationScheduler::new();
        let node = lumen_dom::NodeId::from_index(1usize);
        let style = make_style_with_anim("slide", 1.0);
        sched.sync(node, &style, 0.0);
        assert_eq!(sched.start_times.len(), 1);
        assert!(sched.start_times.contains_key(&(node, 0)));
    }

    #[test]
    fn scheduler_sync_none_not_registered() {
        let mut sched = AnimationScheduler::new();
        let node = lumen_dom::NodeId::from_index(1usize);
        let style = make_style_with_anim("none", 1.0);
        sched.sync(node, &style, 0.0);
        assert!(sched.start_times.is_empty());
    }

    #[test]
    fn scheduler_sync_does_not_reset_existing() {
        let mut sched = AnimationScheduler::new();
        let node = lumen_dom::NodeId::from_index(1usize);
        let style = make_style_with_anim("fade", 1.0);
        sched.sync(node, &style, 0.0);
        // Sync again at t=5 — start time must stay at 0.0.
        sched.sync(node, &style, 5.0);
        assert_eq!(*sched.start_times.get(&(node, 0)).unwrap(), 0.0);
    }

    #[test]
    fn scheduler_remove_node_clears_all_entries() {
        let mut sched = AnimationScheduler::new();
        let node = lumen_dom::NodeId::from_index(1usize);
        let mut style = make_style_with_anim("a", 1.0);
        style.animation_names.push("b".to_string());
        style.animation_durations.push(2.0);
        sched.sync(node, &style, 0.0);
        assert_eq!(sched.start_times.len(), 2);
        sched.remove_node(node);
        assert!(sched.start_times.is_empty());
    }

    #[test]
    fn scheduler_tick_no_keyframes_empty_frame() {
        let mut sched = AnimationScheduler::new();
        let node = lumen_dom::NodeId::from_index(1usize);
        let style = make_style_with_anim("unknown", 1.0);
        sched.sync(node, &style, 0.0);
        let sheet = lumen_css_parser::Stylesheet::default();
        let frame = sched.tick(&sheet, |_| Some(make_style_with_anim("unknown", 1.0)), 0.5);
        assert!(!frame.has_active);
        assert!(frame.overrides.is_empty());
    }

    #[test]
    fn scheduler_tick_opacity_midpoint() {
        let mut sched = AnimationScheduler::new();
        let node = lumen_dom::NodeId::from_index(2usize);
        let style = make_style_with_anim("fade", 1.0);
        sched.sync(node, &style, 0.0);
        let sheet = make_sheet_opacity("fade");
        // At t=0.5s the animation is half-way through → opacity ≈ 0.5.
        let frame = sched.tick(&sheet, |_| Some(make_style_with_anim("fade", 1.0)), 0.5);
        assert!(frame.has_active);
        let entry = frame.overrides.get(&node).expect("node should have overrides");
        let op = entry.opacity.expect("opacity should be set");
        assert!((op - 0.5).abs() < 0.01, "expected ~0.5, got {op}");
    }

    #[test]
    fn scheduler_tick_opacity_at_start() {
        let mut sched = AnimationScheduler::new();
        let node = lumen_dom::NodeId::from_index(3usize);
        let style = make_style_with_anim("fade", 2.0);
        sched.sync(node, &style, 0.0);
        let sheet = make_sheet_opacity("fade");
        let frame = sched.tick(&sheet, |_| Some(make_style_with_anim("fade", 2.0)), 0.0);
        assert!(frame.has_active);
        let op = frame.overrides[&node].opacity.unwrap();
        assert!(op < 0.05, "expected ~0.0 at start, got {op}");
    }

    #[test]
    fn scheduler_tick_opacity_after_end_no_fill() {
        let mut sched = AnimationScheduler::new();
        let node = lumen_dom::NodeId::from_index(4usize);
        let style = make_style_with_anim("fade", 1.0);
        sched.sync(node, &style, 0.0);
        let sheet = make_sheet_opacity("fade");
        // t=2.0 > duration=1.0, no fill-mode → animation ended, no override.
        let frame = sched.tick(&sheet, |_| Some(make_style_with_anim("fade", 1.0)), 2.0);
        assert!(!frame.has_active);
        assert!(frame.overrides.is_empty());
    }

    #[test]
    fn scheduler_tick_direction_reverse() {
        let mut sched = AnimationScheduler::new();
        let node = lumen_dom::NodeId::from_index(5usize);
        let mut style = make_style_with_anim("fade", 1.0);
        style.animation_directions = vec![crate::style::AnimationDirection::Reverse];
        sched.sync(node, &style, 0.0);
        let sheet = make_sheet_opacity("fade");
        let frame = sched.tick(
            &sheet,
            move |_| {
                let mut s = make_style_with_anim("fade", 1.0);
                s.animation_directions = vec![crate::style::AnimationDirection::Reverse];
                Some(s)
            },
            0.25,
        );
        assert!(frame.has_active);
        // Reverse: at t=0.25 raw → effective t=0.75 → opacity≈0.75 (from 0→1, reversed).
        let op = frame.overrides[&node].opacity.unwrap();
        assert!((op - 0.75).abs() < 0.02, "expected ~0.75, got {op}");
    }

    // ─────── TransitionScheduler ───────

    fn make_opacity_transition_style(opacity: f32, duration: f32) -> ComputedStyle {
        let mut s = ComputedStyle::root();
        s.opacity = opacity;
        s.transition_properties = vec!["opacity".to_string()];
        s.transition_durations = vec![duration];
        s.transition_timing_functions = vec![crate::style::TimingFunction::Linear];
        s
    }

    #[test]
    fn transition_scheduler_sync_registers_on_change() {
        let mut sched = TransitionScheduler::new();
        let node = lumen_dom::NodeId::from_index(10usize);
        let old = make_opacity_transition_style(0.0, 1.0);
        let new = make_opacity_transition_style(1.0, 1.0);
        sched.sync(node, &old, &new, 0.0);
        assert_eq!(sched.active.len(), 1);
    }

    #[test]
    fn transition_scheduler_sync_skips_unchanged() {
        let mut sched = TransitionScheduler::new();
        let node = lumen_dom::NodeId::from_index(11usize);
        let style = make_opacity_transition_style(0.5, 1.0);
        sched.sync(node, &style, &style, 0.0);
        assert!(sched.active.is_empty());
    }

    #[test]
    fn transition_scheduler_tick_midpoint() {
        let mut sched = TransitionScheduler::new();
        let node = lumen_dom::NodeId::from_index(12usize);
        let old = make_opacity_transition_style(0.0, 1.0);
        let new = make_opacity_transition_style(1.0, 1.0);
        sched.sync(node, &old, &new, 0.0);
        let frame = sched.tick(0.5);
        assert!(frame.has_active);
        let op = frame.overrides[&node].opacity.unwrap();
        assert!((op - 0.5).abs() < 0.01, "expected ~0.5, got {op}");
    }

    #[test]
    fn transition_scheduler_tick_after_end_removes_entry() {
        let mut sched = TransitionScheduler::new();
        let node = lumen_dom::NodeId::from_index(13usize);
        let old = make_opacity_transition_style(0.0, 1.0);
        let new = make_opacity_transition_style(1.0, 1.0);
        sched.sync(node, &old, &new, 0.0);
        let frame = sched.tick(2.0); // past duration=1.0
        assert!(!frame.has_active);
        assert!(frame.overrides.is_empty());
        assert!(sched.active.is_empty());
    }

    #[test]
    fn transition_scheduler_remove_node_clears_state() {
        let mut sched = TransitionScheduler::new();
        let node = lumen_dom::NodeId::from_index(14usize);
        let old = make_opacity_transition_style(0.0, 1.0);
        let new = make_opacity_transition_style(1.0, 1.0);
        sched.sync(node, &old, &new, 0.0);
        sched.remove_node(node);
        assert!(sched.active.is_empty());
    }

    #[test]
    fn transition_scheduler_delay_no_override_yet() {
        let mut sched = TransitionScheduler::new();
        let node = lumen_dom::NodeId::from_index(15usize);
        let mut old = make_opacity_transition_style(0.0, 1.0);
        old.transition_delays = vec![0.5];
        let mut new = make_opacity_transition_style(1.0, 1.0);
        new.transition_delays = vec![0.5];
        sched.sync(node, &old, &new, 0.0);
        // At t=0.3 we are still inside the delay — no override.
        let frame = sched.tick(0.3);
        assert!(frame.has_active);
        assert!(!frame.overrides.contains_key(&node));
    }

    #[test]
    fn transition_scheduler_interrupted_transition_detection() {
        let mut sched = TransitionScheduler::new();
        let node = lumen_dom::NodeId::from_index(20usize);

        // Start: 0% opacity
        let s0 = make_opacity_transition_style(0.0, 2.0);
        let s1 = make_opacity_transition_style(1.0, 2.0);
        sched.sync(node, &s0, &s1, 0.0);
        assert_eq!(sched.active.len(), 1);

        // At t=1.0 (halfway through), interrupt with a new transition
        let s2 = make_opacity_transition_style(0.5, 2.0);
        sched.sync(node, &s1, &s2, 1.0);

        // The active transition should be updated (interrupted value captured)
        assert_eq!(sched.active.len(), 1);
        let state = sched.active.iter().next().unwrap().1;
        assert!(matches!(state.interrupted_value, Some(AnimValue::Number(_))));
    }

    #[test]
    fn transition_scheduler_fill_mode_forwards_preserves_end_value() {
        let mut sched = TransitionScheduler::new();
        let node = lumen_dom::NodeId::from_index(21usize);
        let old = make_opacity_transition_style(0.0, 0.5);
        let new = make_opacity_transition_style(1.0, 0.5);
        sched.sync(node, &old, &new, 0.0);

        // Set fill_mode to Forwards manually for this test
        if let Some(state) = sched.active.get_mut(&(node, "opacity".to_string())) {
            state.fill_mode = AnimationFillMode::Forwards;
        }

        // At t=1.0 (after duration), should preserve the end value
        let frame = sched.tick(1.0);
        assert!(frame.has_active); // Still active due to fill-mode
        let op = frame.overrides[&node].opacity.unwrap();
        assert!((op - 1.0).abs() < 0.01, "expected ~1.0, got {op}");
    }

    #[test]
    fn transition_scheduler_fill_mode_backwards_applies_start_before_delay() {
        let mut sched = TransitionScheduler::new();
        let node = lumen_dom::NodeId::from_index(22usize);
        let mut old = make_opacity_transition_style(0.0, 0.5);
        old.transition_delays = vec![0.5];
        let mut new = make_opacity_transition_style(1.0, 0.5);
        new.transition_delays = vec![0.5];
        sched.sync(node, &old, &new, 0.0);

        // Set fill_mode to Backwards manually for this test
        if let Some(state) = sched.active.get_mut(&(node, "opacity".to_string())) {
            state.fill_mode = AnimationFillMode::Backwards;
        }

        // At t=0.1 (during delay), should apply the start value (0.0) due to fill-mode
        let frame = sched.tick(0.1);
        assert!(frame.has_active);
        let op = frame.overrides[&node].opacity.unwrap();
        assert!((op - 0.0).abs() < 0.01, "expected ~0.0, got {op}");
    }
}
