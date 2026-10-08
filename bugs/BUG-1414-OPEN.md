# BUG-1414 — grid: item с `width` + `padding`, растянутый по строке, теряет padding в `rect.width`

**Статус:** OPEN
**Заведён:** 2026-10-08 (P6, при BUG-1313)
**Область:** layout (`crates/engine/layout/src/box_tree/grid_trampoline.rs` — финальная раскладка item'а, растянутого по block-оси)

## Симптом

`--dump-layout`:

```html
<style>.g{display:grid;grid-template-columns:100px;grid-template-rows:10px}.a{width:60px;padding-left:6px}</style>
<div class="g"><div class="a"></div></div>
```

Получено: `rect.width = 60` (padding потерян). Ожидается 66 (content-box: 60 + padding 6).

Тот же item при `height: 3px`, без `grid-template-rows` или вне grid — 66. То есть дефект связан с
`align-self: stretch` по block-оси: item перекладывается под высоту строки и ширина берётся как border-box.

## Как найдено

`css/css-grid/grid-items/grid-items-minimum-width-001.html` (BUG-1313): 14 сабтестов с padding/border
(`expected 66 but got 60`, `63/69/62/64/75`) — `grid: 10px 10px / minmax(auto, 0px)`.

## Как проверить

`grid-items-minimum-width-001.html` — все сабтесты `.paddingLeft6`/`.paddingRight3`/`.borderLeft2`/`.borderRight4`.
