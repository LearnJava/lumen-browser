# BUG-1292 — процентные значения в `translate()`, `translateX/Y/3d()` и свойстве `translate` вычисляются как 0

**Статус:** OPEN
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
