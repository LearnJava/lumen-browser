# BUG-1405 — `clip-path`: `circle()`/`ellipse()` без радиуса и с `closest-side`/`farthest-side`, `inset(… round …)`, `<geometry-box>` игнорируются или считаются неверно

**Статус:** FIXED 2026-10-22 (P6)
**Заведён:** 2026-10-07 (P2, WPT-RUN-14 срез 18, `css/css-fonts` + `css/css-masking` + `css/WOFF2`)
**Область:** layout (`crates/engine/layout/src/style/parse/shape.rs::parse_clip_path`, `display_list` — вычисление опорного прямоугольника `clip-path`)

## Симптом

`--screenshot`, 800×600, `<div style="width:200px;height:200px;background:green;clip-path:…">`, число зелёных пикселей
(`.tmp/s18/cnt.py`):

| `clip-path` | получено | ожидается |
|---|---|---|
| без `clip-path` | 40 000 | 40 000 |
| `circle()` | **40 000** | 31 416 (`closest-side`, r = 100) |
| `circle(closest-side)`, `circle(farthest-side)`, `circle(at 50% 50%)` | **40 000** | 31 416 / обрезка по углам / 31 416 |
| `ellipse()`, `ellipse(closest-side closest-side)` | **40 000** | 31 416 |
| `circle(50%)`, `circle(100px)` | 31 484 | 31 416 (верно) |
| `circle(50px)` | 7 888, bbox (50,50)…(149,149) | 7 854 (верно) |
| `inset(10px)` | 32 400 | 32 400 (верно) |
| `inset(10px round 20px)` | **28 800**, bbox (20,10)…(179,189) | ≈ 32 057, bbox (10,10)…(189,189) |

`<geometry-box>` — блок `width:100px; height:100px; padding:10px 30px 50px 70px; border:5px solid; margin:20px`:

| `clip-path` | получено | ожидается |
|---|---|---|
| `circle(20px at 50% 50%)` | центр (127,102), 1 275 px | то же (border-box по умолчанию) |
| `circle(20px at 50% 50%) content-box` | **то же, центр (127,102)** | центр content-box (145,85) |
| `circle(20px at 50% 50%) margin-box` | **то же** | центр margin-box |
| `padding-box` / `content-box` / `border-box` без формы | **25 600 px, без обрезки** | обрезка по соответствующему боксу (19 600 / 10 000 / 25 600) |

`parse_clip_path` (`shape.rs:27`) разбирает функцию по `s.find('(')`…`s.rfind(')')`, поэтому хвост `content-box` после
`)` отбрасывается, а строка без скобок (`padding-box`) не разбирается вовсе. Радиус у `circle`/`ellipse` —
`parse_shape_value(radius_part.trim())?`: пустой аргумент и ключевые слова дают `None`, декларация молча
отбрасывается.

## Как найдено

WPT-RUN-14 срез 18, `css/css-masking/clip-path/`: `clip-path-circle-001…009`, `-circle-closest-corner`,
`-circle-farthest-corner`, `clip-path-ellipse-001…008`, `-ellipse-closest-farthest-corner` (22 id, все `thick`),
`clip-path-{borderBox,contentBox,paddingBox,marginBox,fillBox,strokeBox,viewBox}-*`, `clip-path-geometryBox-2`,
`clip-path-reference-box-004` (29 id, все `thick`). Часть id может падать и по другим причинам: счёт — по имени и
тексту теста, не по проверке каждого файла.

## Что делать

В `parse_clip_path` принимать `<geometry-box>` до и после функции и как самостоятельное значение, хранить его в
`ClipPath`; разрешать опорный прямоугольник в display list; добавить `closest-side`/`farthest-side` и пустой
радиус (по умолчанию `closest-side`) у `circle`/`ellipse`; для `inset()` разобрать `round <border-radius>`.
`rect()`/`xywh()`/`shape()` — отдельно, см. `CSS-SPECS.md` строку `clip-path`.

## Как проверить

`css/css-masking/clip-path/clip-path-circle-002.html`, `clip-path-ellipse-001.html`, `clip-path-contentBox-1a.html`,
`clip-path-borderBox-1c.html`.

## Повторное измерение: WPT-RUN-14 срез 23 (2026-10-08)

Тот же строковый разбор аргументов форм действует и у `shape-outside` (`parse_circle_px`, `parse_shape_ellipse_px` в `box_tree/shapes_floats.rs`): `circle()`, `circle(closest-side)`, `circle(50%)`, `ellipse(50% 50%)` без `at`, `at left top` дают прямоугольное обтекание, у `circle()` `at <position>` отбрасывается (BUG-1517, 25 reftest). Исправлять разбор аргументов нужно один раз для `clip-path` и `shape-outside`.

## Исправление (2026-10-22, P6)

- `parse_clip_path` (`style/parse/shape.rs`) разбирает `[<basic-shape> || <geometry-box>]`: бокс до или после формы и
  голый бокс. Новые варианты `ClipPath::Boxed { shape, geometry }` и `ClipPath::InsetRound`, `GeometryBox`,
  `ShapeValue::{ClosestSide, FarthestSide}`.
- `circle()`/`ellipse()` без радиуса — `closest-side`; `closest-side`/`farthest-side`; `at <position>` из 1–2 значений
  с ключевыми словами (в том числе `circle(at left top)` без радиуса). Трёх- и четырёхзначная позиция
  (`at right 10px bottom 20px`) по-прежнему не поддержана — декларация отбрасывается.
- `inset(… round <r>{1,4} [/ <r>{1,4}])` — многоугольник с дугами в углах (16 сегментов на угол), радиусы
  масштабируются по CSS Backgrounds L3 §5.5.
- paint: `display_list/clip_shape.rs` (форма вынесена из `display_list.rs`); `ClipRefs` даёт border/padding/content/margin
  box элемента. Для CSS-боксов `fill-box` = content-box, `stroke-box`/`view-box` = border-box (как в `mask-clip`).
- Попутно `cpu_raster.rs`, `DrawOutline`: решение «клип можно не применять» принималось по рамке без полосы
  обводки, поэтому `outline` шире клипа вылезал за `clip-path`/`overflow` (`clip-path-marginBox-1b`).

Пиксельная проверка (`--screenshot`, 200×200, зелёный блок): `circle()`/`ellipse()` — 31 192 px (идеал 31 416), `circle(50px)` —
7 772, `inset(10px round 20px)` — 32 024 при bbox (10,10)…(189,189); `content-box` сдвигает центр круга на border+padding,
`padding-box`/`content-box` без формы режут по своему боксу.

## Остаток

- WPT `css/css-masking/clip-path` (локальное сравнение тест/референс с допуском `fuzzy`, 50 id этой выборки): 14 проходят
  (до правки — единицы). Не проходят: ≈17 id на корневом `<svg>` — BUG-1680 (раскладка `<svg>` с padding/border);
  `clip-path-circle-00N`/`ellipse-00N` — край окружности: 635 px с макс. расхождением 128 при допуске 60 (качество
  антиалиасинга маски `PushClipPath` в CPU-растре, форма и положение верны); `*-closest-corner`/`farthest-corner` —
  ключевые слова удалённой из спецификации формы, не разбираются.
- `rect()`/`xywh()`/`shape()` — отдельно, `CSS-SPECS.md`.
- `shape-outside`: тот же строковый разбор аргументов в `box_tree/shapes_floats.rs` — BUG-1517 (общий парсер не вынесен).
