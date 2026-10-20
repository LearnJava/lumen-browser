# BUG-1413 — Каскад: внешний `<link rel=stylesheet>` всегда перебивает `<style>`, даже если `<style>` стоит позже в документе

**Статус:** OPEN
**Заведён:** 2026-10-08 (P1, GRID-BASELINE-2)
**Область:** shell (`crates/shell/src/page_pipeline.rs` — сборка `css` в `build_page_cascade`; `crates/shell/src/stylesheets.rs::load_linked_stylesheets`; `crates/shell/src/relayout.rs` — пересборка из `DynamicCssBase`; `crates/shell/src/doc_extract.rs::DynamicCssBase`)

## Симптом

Порядок листов в каскаде — «сначала все инлайновые `<style>`, потом все `<link>`», а не порядок документа
(CSS Cascade L4 §6.4.3 «Order of Appearance», HTML LS §4.2.4: порядок — по положению элементов в дереве).
Правило из `<style>`, стоящего ПОСЛЕ `<link>`, проигрывает правилу из `<link>` при равной специфичности.

`--dump-layout`:

```html
<link href="ord.css" rel="stylesheet">          <!-- .a { background-color: red } -->
<style>.a { background-color: blue }</style>
<div class="a">x</div>
```

Получено: `bg=#ff0000ff` (красный, лист из `<link>`); ожидается синий.

Тот же дефект с `display`: `<link href="/css/support/grid.css">` (`.grid { display: grid }`) + `<style>.grid { display: inline-grid }</style>` →
`getComputedStyle(el).display === "grid"`, ожидается `inline-grid`. Из-за этого в harness не проходит
`css/css-grid/alignment/grid-baseline-004.html` (18 сабтестов, все три `.grid` встают столбцом, высота контейнера
`280` вместо `120`) и любой WPT-тест с `<link>` на вспомогательный CSS и собственными переопределениями.

## Почему

`page_pipeline.rs` собирает `css = <@import-префикс> + <все inline> + <все linked>` (`extract_style_blocks` склеивает
тексты всех `<style>`, `load_linked_stylesheets` — всех `<link>`), и `relayout.rs` при поздней вставке `<style>`
повторяет ту же склейку из `DynamicCssBase { imports_prefix, linked, … }`. Порядок документа между двумя видами
листов теряется.

## Что делать

Собирать текст в порядке документа: `extract_style_blocks` и `load_linked_stylesheets` отдают куски по узлам
(`Vec<(NodeId, String)>`), сборка идёт одним обходом дерева; `@import` каждого `<style>` — перед текстом этого же
`<style>`, а не одним префиксом на весь документ; `DynamicCssBase` хранит куски по узлам, чтобы пересборка
после правки `<style>` шла в том же порядке. Правка двигает пиксели на страницах, где инлайновый `<style>`
переопределяет внешний лист, — полный `graphic_tests/run.py --continue-on-fail` и новые эталоны в том же коммите.

## Как проверить

Страница выше; `css/css-grid/alignment/grid-baseline-004.html`: после правки в harness сойдутся высоты контейнеров (120/165/250) и y-смещения grid'ов; `x`-смещения (160 вместо 152) зависят ещё и от [BUG-1306](BUG-1306-OPEN.md) (ширина пробела — из оценщика текста, не из Ahem).
