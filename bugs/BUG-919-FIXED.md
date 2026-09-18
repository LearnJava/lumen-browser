# BUG-919 — `<details open>` внутри документа от `DOMParser`/`innerHTML` не порождает `toggle`

**Статус:** FIXED 2026-09-18 (P3)
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

## Исправлено

Три независимых входа, каждый со своим `toggle`-долгом:

1. **Живой документ, `innerHTML`/`insertAdjacentHTML`.** `_lumen_set_inner_html`
   обёрнут в `web_api_shim_tail_b.js`: после записи, если вставленная разметка
   похожа на `<details` (дешёвый regex-префильтр, полный документ не гоняется
   без нужды), зовутся `_lumen_details_open_scan()`
   (parser-owed `toggle`) и `_lumen_details_connected_scan()` (exclusivity).
2. **Живой документ, вставка узла (`appendChild`/`insertBefore`).**
   Те же нативы обёрнуты аналогично: `details-name-exclusivity-fragment-
   insertion.html` строит `<details open name=x>` ОТДЕЛЬНО от документа
   (`createElement` + `setAttribute`), поэтому exclusivity-проверка в момент
   записи атрибута не находит конфликта — оба элемента ещё не связаны. Новый
   `_lumen_details_connected_scan()` повторно проверяет exclusivity для каждого
   уже подключённого `<details open>`, независимо от карты «событие уже
   учтено», — действует только на настоящий конфликт имени, поэтому
   безопасен как повторный проход по уже устоявшемуся дереву.
3. **Виртуальный документ `DOMParser.parseFromString`.** У него нет нативного
   узла за спиной вовсе — собственный токенизатор `dom_parser.rs` пишет
   атрибуты как обычную JS-запись, поэтому получил свою копию шага «queue a
   details toggle event task» (`_vDetailsOpenScan`, вызывается из
   `_vBuildDocument` и из `innerHTML`-сеттера `VElement`). Добавлена
   минимальная модель `EventTarget` на `VNode.prototype`
   (`addEventListener`/`removeEventListener`/`dispatchEvent`) — раньше это были
   Phase-0 no-op заглушки, а `toggleEvent.html`'s parser-подтест вешает
   `ontoggle` сразу после `parseFromString`.

Попутно найден и закрыт в той же сессии native-уровневый дефект: пре-фильтр
`_lumen_subtree_has_details` (шаг 1/2 выше) может дойти до
`_lumen_query_selector_scoped`/`_lumen_query_selector_all_scoped` с
устаревшим/чужим `NodeId` (тот же класс входа, что и [BUG-986](BUG-986-FIXED.md)
для `_lumen_append_child`) — обе регистрации в `dom_core.rs` не были
защищены `doc.contains_id`, в отличие от большинства соседних нативов, и
паниковали в `query_all_within`. Добавлен тот же guard, что уже стоит на
`_lumen_append_child`/`_lumen_remove_child` (no-op вместо паники).

Тесты: `cargo test -p lumen-js --profile dev-release --features v8-backend
--lib` — 3873/3873 зелёных (три новых: `details_fragment_insertion_closes_-
detached_open_sibling`, `details_inner_html_open_fires_toggle`,
`dom_parser_details_open_fires_toggle_task_not_sync`). `cargo clippy -p
lumen-js --all-targets --features v8-backend -- -D warnings` чист.
`cargo test -p lumen-layout --profile dev-release --lib deep_chain` (независимая
проверка `scripts/scoped-test.sh`, которая в `debug`-профиле упирается в
таймаут на этих же тестах) — 14/14 зелёных, к правке не относится.
