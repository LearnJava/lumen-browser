# BUG-1277 — `white-space: nowrap` не мешает переносу между атомарными inline-боксами (`inline-flex`)

**Статус:** OPEN
**Заведён:** 2026-10-05 (P1, FLEX-VWM-5 — найдено при A/B reftest `css/css-flexbox`)
**Область:** layout (inline-раскладка: перенос строк между атомарными inline-боксами)

## Симптом

```html
<div style="width:100px;overflow:scroll;white-space:nowrap">
  <div style="display:inline-flex;width:40px;height:20px;margin:3px">1</div>
  <div style="display:inline-flex;width:40px;height:20px;margin:3px">2</div>
  <div style="display:inline-flex;width:40px;height:20px;margin:3px">3</div>
</div>
```

`--dump-layout`: первые два на `y=3`, третий на `y=29` (вторая строка). Ожидается одна строка, третий уходит в переполнение
(CSS Text L3 §3: `nowrap` запрещает мягкие переносы, в том числе между атомарными inline-боксами).

## Как найдено

`css/css-flexbox/scrollbars.html`, `scrollbars-auto.html`, `scrollbars-no-margin.html`: ref собран на `display: inline-flex` +
`white-space: nowrap` (`.row > div`), тест — на `display: flex` с `overflow: scroll`. Раньше все три числились `identical`:
вертикальные заголовки `.container-row` (flex-ряд, `align-items: flex-start`) заполняли окно по высоте, и сравниваемая часть кадра
не содержала ни одного `.flex`. После FLEX-VWM-5 заголовки обтягивают содержимое, `.flex` попадает в кадр, и расхождение
(перенос в ref) видно.

## Как проверить

Страница выше с `lumen --dump-layout`; затем `css/css-flexbox/scrollbars.html` через `tests/wpt/reftest_pixdiff.py`.
