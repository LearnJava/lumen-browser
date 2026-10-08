# BUG-1316 — у живого `document` нет `children`/`childElementCount`/`firstElementChild`/`lastElementChild`

**Статус:** FIXED 2026-10-07 (P6)
**Заведён:** 2026-10-06 (P2, WPT-RUN-14 срез 8, `css/css-grid`, вторая половина)
**Область:** js — `crates/js/src/shim/web_api_shim_mid.js`. У отсоединённого документа (`_lumen_build_detached_document`, `:5211-5240`) аксессоры `children`/`childElementCount`/`firstElementChild`/`lastElementChild` определены (BUG-1161), у живого `document` — нет, и в `Document.prototype` их нет (DOM LS §4.2.6: `Document` реализует `ParentNode`).

## Симптом

Страница-проба под `run_report.py` (testharness):

```
typeof document.children            -> "undefined"
typeof document.childElementCount   -> "undefined"
typeof document.firstElementChild   -> "undefined"
typeof document.lastElementChild    -> "undefined"
'children' in Document.prototype    -> false
typeof ParentNode                   -> "undefined"
typeof document.documentElement.children -> "object"
typeof document.append / prepend / replaceChildren / querySelector -> "function"
```

Остальные члены `ParentNode` у документа есть, нет только четырёх свойств-аксессоров (и интерфейсного объекта `ParentNode`).

## Как найдено

WPT-RUN-14 срез 8, `css/css-grid/parsing/`: `grid-template-areas-one-cell.html` (6 сабтестов), `grid-template-shorthand-areas-valid.html` (6), `grid-template-shorthand-composition.html` (11) — `const root = document.children[0]; root.style.gridTemplateAreas = …` падает `Cannot read properties of undefined (reading '0')`. `grep -rlE` по `tests/wpt` (без `tools/`) находит приём `document.children`/`childElementCount`/`firstElementChild`/`lastElementChild` ровно в этих трёх файлах.

## Что делать

Определить четыре свойства на `Document.prototype` (или на живом `document`) по образцу отсоединённого документа: `children` — живой `HTMLCollection` элементов-детей (сейчас у `documentElement` он есть — использовать ту же фабрику), `childElementCount`, `firstElementChild`, `lastElementChild`. Глобал `ParentNode` — отдельно, если нужен `instanceof`.

## Как проверить

`css/css-grid/parsing/grid-template-areas-one-cell.html`; одной строкой `document.children.length === 1` на странице с одним `<html>`.

## Решение (2026-10-07, P6)

Четыре аксессора добавлены в объектный литерал живого `document` (`web_api_shim_mid.js`, рядом с `firstChild`/`lastChild`): `children` — `_lumen_make_html_collection(_lumen_root_nid)`, `childElementCount`/`firstElementChild`/`lastElementChild` — через `_lumen_element_child_nids`. Литерал не наследует `Document.prototype`, поэтому свойства собственные (как `firstChild`, `hasChildNodes`). Глобал `ParentNode` не заводился — в WPT-кластере он не нужен.

Побочный дефект, вскрытый реальным прогоном: `_lumen_is_element_nid` считал doctype элементом (его `tagName` — `html`, без `#`), и на странице с `<!DOCTYPE html>` `document.children[0]` оказывался doctype'ом — `style` на нём пуст, `=== document.documentElement` ложно. Юнит-фикстура `make_doc()` doctype не содержит, поэтому первый тест это не ловил; добавлен второй, с doctype. Исправление — `_lumen_is_doctype` в `_lumen_is_element_nid` (затрагивает и `ParentNode`-обход у остальных узлов, где doctype раньше мог попасть в `children`).

Тесты: `crates/js/src/dom/tests/v8_bug1316_document_parentnode.rs`. WPT: `grid-template-areas-one-cell.html` 6/6; `grid-template-shorthand-composition.html` и `grid-template-shorthand-areas-valid.html` перестали падать на `Cannot read properties of undefined`, часть сабтестов остаётся FAIL по другим причинам (ожидания в `.ini` обновлены).
