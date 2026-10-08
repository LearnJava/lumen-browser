# BUG-1496 — `@container style(--x: y)` срабатывает только на предке с `container-type`; по спецификации любой элемент — style-контейнер

**Статус:** OPEN
**Заведён:** 2026-10-08 (P2, WPT-RUN-14 срез 22, `css/css-view-transitions` + `css-conditional` + `css-variables` + `css-properties-values-api` + `css-mixins`)
**Область:** layout (`crates/engine/layout/src/style/container.rs`, `crates/engine/layout/src/box_tree/container_anchor.rs` — выбор контейнера для `style()`)

## Симптом

CSS Conditional 5 §container-queries: запрос `style()` вычисляется на ближайшем предке, у которого есть контейнер-имя, а без имени — на ближайшем предке-элементе вообще (любой элемент является style-контейнером). У нас `style()` находит только элемент с `container-type`. Без `container-type` (с `container-name` или без) — не срабатывает, ни на родителе, ни на самом элементе, ни на `:root`. Отдельно: boolean-форма `style(--x)` и `style(color:red)` верны, когда `container-type` есть. Сопутствующее: 22 теста `container-queries/*style*` завершаются `ERROR` — `assert_implements_style_container_queries()` читает `CSSStyleSheet.replaceSync('@container STYLE(--foo: bar){}').cssRules[0].containerQuery`, а `cssRules` у такого листа пуст (BUG-1455) — после CSSOM-10 они откроются и покажут эту причину.

## Проба

Проба (`--mcp`, цвет `#t`, `@container ⟨q⟩{#t{color:green}}`):

| контейнер | запрос | у нас | ожидается |
|---|---|---|---|
| `.c{--x:y}` (без `container-type`) | `style(--x: y)` | `red` | `green` |
| `.c{container-name:n;--x:y}` | `n style(--x: y)` | `red` | `green` |
| `<div style="--x:y"><div id=t>` | `style(--x: y)` | `red` | `green` |
| `:root{--x:y}` | `style(--x: y)` | `red` | `green` |
| `.c{container-type:inline-size;--x:y}` | `style(--x: y)` | `green` | `green` |
| `.c{container-type:size;…;--x:y}` | `(min-width:100px) and style(--x:y)` | `green` | `green` |
| `.c{color:red;container-type:size}` | `style(color:red)` | `green` | `green` |

## Как найдено

WPT-RUN-14 срез 22: `css-conditional/container-queries/{custom-property-style-queries,style-query-*,style-container-*,multiple-style-containers-comma-separated-queries,registered-color-style-queries}` — 36 id (22 ERROR, 12 OK с падениями, 2 FAIL).

## Что делать

Определять style-контейнер как ближайший предок-элемент без требования `container-type` (для запроса с именем — с совпавшим `container-name`); `size`-запросы по-прежнему требуют `container-type`. Затем CSSOM-10 (BUG-1455) для `containerQuery`.

## Как проверить

Таблица выше; `css/css-conditional/container-queries/style-query-no-cycle.html`, `custom-property-style-queries.html`.
