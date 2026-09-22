# BUG-1094 — SVG2 геометрические/красящие CSS-свойства (`cx`/`cy`/`r`/`rx`/`ry`/`x`/`y`, `color-interpolation`, `path-length`) не в `SUPPORTED_PROPERTIES`: `CSS.supports()` и интерполяция полностью не работают

**Статус:** OPEN
**Тип:** пробел реализации — SVG2 переопределил presentation-атрибуты геометрии как настоящие CSS-свойства, Lumen их не знает вовсе.
**Заведён:** 2026-09-22 (P2, WPT-RUN-7 срез 53, `svg`)
**Область:** css-parser — `crates/engine/css-parser/src/lib.rs::SUPPORTED_PROPERTIES` (нет `cx`/`cy`/`r`/`rx`/`ry`/`x`/`y`/`color-interpolation`/`path-length`; подтверждено `grep`, 0 совпадений на все девять имён)
**Владелец:** P4 (css-parser/layout).

## Симптом

9 файлов `svg/geometry/animations/{cx,cy,r,rx,ry,x,y}-composition.html`, `svg/painting/color-interpolation-animation.html`, `svg/path/animations/path-length-interpolation.tentative.html` — **все 284 подтеста всех девяти файлов падают на первой же проверке `test_interpolation`** (`tests/wpt/css/support/interpolation-testcommon.js:396`, `assert_true(CSS.supports(property, from), '\'from\' value should be supported')`), 0/284 pass, каждый файл 100 % `FAIL`:

```
cx-composition.html   30 fail / 30
cy-composition.html   30 fail / 30
r-composition.html    30 fail / 30
rx-composition.html   30 fail / 30
ry-composition.html   30 fail / 30
x-composition.html    30 fail / 30
y-composition.html    30 fail / 30
color-interpolation-animation.html  42 fail / 42
path-length-interpolation.tentative.html  32 fail / 32
```

Каждый файл гейтит себя `CSS.supports(<имя_свойства>, <значение>)` перед тем, как вообще регистрировать сравнение интерполяции — поэтому дефект блокирует не только CSS.supports-проверку, а весь файл целиком: assert бросает исключение, остальной тест-кейс не выполняется.

## Причина

SVG2 §Geometry Properties/§Presentation Attributes переносит `cx`/`cy`/`r`/`rx`/`ry`/`x`/`y` (уже существовавшие как SVG presentation-атрибуты) в разряд настоящих CSS-свойств — животрепещущий момент для `CSS.supports()`, каскада и CSS Animations/Transitions на этих значениях. `color-interpolation` (SVG2 §Painting) и `path-length` (SVG2 §Geometry, зеркало атрибута `pathLength`) — из того же семейства. Ни одно из девяти имён не в `SUPPORTED_PROPERTIES` (`crates/engine/css-parser/src/lib.rs`), список которого используется и `@supports`, и `CSS.supports()` (док-комментарий над константой это подтверждает), и, вероятно, самим `apply_declaration` в `lumen-layout` (не проверено отдельно — свойства могли частично лечь через presentation-атрибутный путь, но как CSS-свойства из style-блока/через `CSSStyleDeclaration` не работают).

## Ожидание

Все девять имён добавлены в `SUPPORTED_PROPERTIES` + разбор значений в `apply_declaration` (тип `<length>`/`<percentage>` для геометрических, ключевые слова `sRGB`/`linearRGB`/`auto` для `color-interpolation`, `<number>` для `path-length`) + вычисляемое значение, если у соответствующих WPT-файлов есть `-computed.html`-варианты (не проверено — вне скоупа этой пробы). Кандидат для skill `/lumen-add-css-property`, но специфика в том, что эти свойства уже существуют как presentation-атрибуты SVG (`svg.rs`/layout) — цена смешивания CSS-каскада с атрибутным путём (приоритет каскада presentation-атрибут < автор-стиль) требует внимания при реализации, не тривиальное добавление с нуля.

## Связанное

- `docs/tasks/p2-test-track.md#test-3-срез-53-2026-09-22`.

## Не проверялось

- Работают ли эти девять свойств как presentation-атрибуты (`<circle cx="10">`, не CSS) — вероятно да (базовый рендеринг SVG в CAPABILITIES.md заявлен), но не переверено этой пробой: она бьёт только в CSS-путь.
- Полная грамматика каждого свойства по спеке (проценты относительно viewport для `cx`/`r`/…, `auto` для `path-length`) — не выведена, только факт отсутствия в `SUPPORTED_PROPERTIES`.
