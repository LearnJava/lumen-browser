# BUG-1292 — процентные значения в `translate()`, `translateX/Y/3d()` и свойстве `translate` вычисляются как 0

**Статус:** FIXED 2026-10-19 (P3)
**Заведён:** 2026-10-06 (P2, WPT-RUN-14 срез 5, `css/css-transforms`)
**Область:** layout (`crates/engine/layout/src/style/parse/transform.rs::parse_transform_fn` — `parse_length_px`; `TransformFn::Translate(f32, f32)` хранит px)

## Симптом

`--screenshot`/`--dump-display-list`, `position:absolute; width:100px; height:50px`:

| разметка | получено | ожидается |
|---|---|---|
| `transform: translateX(50%)` | `PushTransform` нет, бокс на месте | сдвиг на 50 px |
| `transform: translate(50%, 100%)` | на месте | (50, 50) |
| `transform: translate(50px, 10%)` | `[1 0 0 1 50 0]` — процент по y = 0 | (50, 5) |
| `transform: translateX(calc(50% + 10px))` | на месте | сдвиг на 60 px |
| `transform: translate(10%)`, `translate3d(50%,0,0)` | на месте | сдвиг |
| свойство `translate: 50% 0` | на месте | сдвиг на 50 px |

Процент нельзя вычислить при разборе: ему нужен размер бокса (CSS Transforms L1 §3, «percentages refer to the size of the
reference box»). `TransformFn` хранит только `f32` в px, процентного варианта нет; `getComputedStyle(el).translate`
(тест `translate-getComputedStyle.html`) тоже пуст.

## Как найдено

WPT-RUN-14 срез 5: 35 id `css/css-transforms`, содержащих процентный `translate*` (13 reftest `transform-percent-*`,
`transform-inherit-002`, `animation/*-percent-*`; testharness `transforms-support-calc.html`, `translate-getComputedStyle.html`,
`animation/transform-interpolation-*-value.html`, `parsing/transform-valid.html`).

## Что делать

Хранить `<length-percentage>` в `TransformFn::Translate*` и в поле `translate`; разрешать против reference box
(`transform-box`; для HTML — border box) при построении матрицы в `property_trees.rs`; то же для анимаций
(`animation.rs` интерполирует `TransformFn`).

## Как проверить

`css/css-transforms/transform-percent-0{01…10}.html`, `transforms-support-calc.html`, `translate-getComputedStyle.html`.

## Исправление (2026-10-19, P3)

- `TransformFn::TranslateLp(Length, Length, f32)` (`style/values/transform.rs`) — `translate()`/`translateX/Y()`/`translate3d()`
  с процентом или `calc()`/`min()`/… в X/Y. Чисто px-значения остались в прежних вариантах (разбор: `parse_transform_fn` +
  `parse_translate_lp`; запятые внутри `calc()` больше не рвут список аргументов).
- Свойство `translate` теперь `Option<(Length, Length)>` (`ComputedStyle::translate`, `apply/motion.rs`).
- Процент разрешается при построении матрицы против border box самого элемента: `property_trees::TransformRef`
  (ширина/высота/`font-size`) и `translate_lp_matrix`; `forward_box_transform`, `compute_local_transform`,
  `transform_fns_to_matrix` (анимационный override в paint) принимают `TransformRef`.
- Hit-тест (`paint/src/hit_test.rs::invert_box_transform`) теперь инвертирует `forward_box_transform` — раньше он
  игнорировал индивидуальные `translate`/`rotate`/`scale` и motion path.
- Анимация `TranslateLp` ↔ `TranslateLp` — покомпонентно (`animation.rs::blend_length`, разные единицы — через `calc()`).
- `getComputedStyle(el).transform` считает процент от `rect` бокса (`resolved_geometry.rs`).

Проверка: `--screenshot` `css/css-transforms/transform-percent-001…008.html` совпадают с `transform-percent-ref.html`
пиксель в пиксель (до правки — нет); юнит-тесты `filter_transform_snap_mask::{transform_translate_percent_*,
translate_property_percent_and_calc}`, `hit_test::local_point_after_percent_translate_and_translate_property`.
Остаток (SVG-элементы, сериализация `translate`, `calc()` в `rotate`/`scale`) — [BUG-1645](BUG-1645-OPEN.md).
