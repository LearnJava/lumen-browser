# BUG-1166: `getBoundingClientRect` ignores the scroll offset of an ancestor scroll container

**Статус:** OPEN
**Компонент:** js/shell (`_lumen_get_bounding_rect` — прямоугольники из
`update_layout_rects`; `crates/shell/src/relayout.rs`, `crates/js/src/v8_runtime/install/platform.rs`)
**Найден:** P3, при исправлении BUG-627, 2026-09-25

## Симптом

Живой зонд (`--mcp-live-port`, dev-release): `#root { overflow-y: scroll;
height: 200px }` с высоким потомком `#target`. После `root.scrollTop = 150`
(и даже после принудительного relayout через `appendChild` +
`offsetHeight`) `target.getBoundingClientRect().top` не меняется — тот же
результат, что при `scrollTop = 0`. По CSSOM View прямоугольник обязан
сдвинуться на −150.

То же для прокрутки страницы: после `window.scrollTo(0, 500)` (`scrollY`
стал 500) `#root` над высотой `calc(100vh + 100px)` сообщает `top: 1053` —
координату в документе, а не во вьюпорте.

## Масштаб

- Все, кто вычисляет видимость по `getBoundingClientRect` внутри
  прокручиваемого контейнера (виртуальные списки, lazy-load в каруселях,
  sticky-эмуляции).
- `IntersectionObserver` берёт геометрию оттуда же:
  WPT `isIntersecting-change-events.html` («Set scrollTop=100 and check for
  one new notification.») и `scroll-margin-dynamic.html` («Test scroll margin
  intersection after scrolling») падают только из-за этого (BUG-627
  исправил сам наблюдатель).

## Fix shape

Прямоугольники, которые shell отдаёт в `update_layout_rects`, — в
координатах документа без учёта `scroll_x/scroll_y` предков-скроллеров.
Нужно вычитать суммарное смещение прокрутки всех scroll-контейнеров на пути
к корню (как это делает paint через `PushScrollLayer`) либо при сборке
прямоугольников, либо в биндинге `_lumen_get_bounding_rect` по
`_lumen_get_scroll_state` предков — и пересобирать их при изменении
`scrollTop` из скрипта.
