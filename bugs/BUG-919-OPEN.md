# BUG-919 — `<details open>` внутри документа от `DOMParser`/`innerHTML` не порождает `toggle`

**Статус:** OPEN
**Заведён:** 2026-08-25 (P1, остаток [BUG-851](BUG-851-FIXED.md))
**Область:** `crates/js/src/dom.rs` — `_lumen_details_open_scan()` (вызывается
только из `_lumen_apply_ready_state('interactive')`), обёртки над
`_lumen_set_attr`/`_lumen_remove_attr` там же
**Владелец:** P1/P3 (`lumen-js`)

## Симптом

```js
new DOMParser()
  .parseFromString('<details open>', 'text/html')
  .querySelector('details').ontoggle = e => { /* никогда не вызывается */ };
```

Последний подтест
`html/semantics/interactive-elements/the-details-element/toggleEvent.html`
(«Setting open from the parser fires a toggle event») уходит в TIMEOUT, и из-за
него весь файл остаётся TIMEOUT при 10 зелёных подтестах из 11.

## Причина

BUG-851 свёл `toggle` к шагам изменения атрибута `open`, у которых два входа:

1. обёртки над `_lumen_set_attr`/`_lumen_remove_attr` — всё, что пишет атрибут
   из скрипта;
2. `_lumen_details_open_scan()` — один проход по `document` в конце разбора,
   потому что разметку парсер кладёт в арену мимо этих обёрток.

Документ, построенный `DOMParser.parseFromString` (а равно поддерево из
`innerHTML`/`insertAdjacentHTML`), не проходит ни через один из них: разбор идёт
нативным `lumen_html_parser`, а `readyState` такого документа никогда не
переходит в `interactive` — скан для него не запускается вовсе.

## Направление починки (не предписание)

Позвать те же шаги на свежепостроенном поддереве: у `parseFromString` — по
готовому документу, у `innerHTML`/`insertAdjacentHTML` — по вставленным узлам.
Ключ — переиспользовать `_lumen_details_open_changed`, а не заводить третий
вход: `_details_known_open` уже гарантирует «не более одного события на
элемент».

Побочно тем же ходом закрывается вставка `<details open>` из фрагмента в живой
документ, которую `details-name-exclusivity-fragment-insertion.html` проверяет
отдельно (сейчас 0/1).

## Как проверить фикс

`run_report.py --all --root html/semantics/interactive-elements --recursive` —
`toggleEvent.html` должен стать OK 11/11 (а не TIMEOUT 10/11).
