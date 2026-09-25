# BUG-1164 — `NodeIterator`/`TreeWalker`: нет `NodeFilter.SHOW_ATTRIBUTE` и соседей, обход теряет узлы вне дерева документа

**Статус:** OPEN
**Заведён:** 2026-09-25 (P6, при закрытии [BUG-863](BUG-863-FIXED.md))
**Область:** js — `crates/js/src/shim/web_api_shim_mid_b4.js:1070` (`NodeFilter`), `_TreeWalker`
(`:1145`), `_NodeIterator` (`:1314`)

## Симптом

После BUG-863 `dom/traversal` впервые проходит `setup`: `run_report.py --all --root dom/traversal
--recursive --processes 4` — **17/18 harness OK, 1031/1583 сабтестов** (до — 15/18, 26/56).
Классы отказов:

1. **304 FAIL** `.whatToShow expected (undefined) undefined but got (number) N` в
   `NodeIterator.html`: в `NodeFilter` нет `SHOW_ATTRIBUTE` (0x2), `SHOW_ENTITY_REFERENCE` (0x10),
   `SHOW_ENTITY` (0x20), `SHOW_NOTATION` (0x800) — тест передаёт `undefined`, а итератор подставляет
   `0xFFFFFFFF`. `NodeFilter-constants.html` 1/2 по той же причине.
2. `.nextNode()`/`.firstChild()` возвращают `null` там, где ожидается узел (~130 FAIL): корень —
   doctype, PI, комментарий, узел отсоединённого документа или сам `document` с детьми вне арены
   (`_tw_subtree` идёт по арене, а дети отсоединённого документа живут в JS-массиве — см. BUG-1161).
3. 36 FAIL `Cannot read properties of undefined (reading 'previousSibling')` — `TreeWalker` на
   PI из `createProcessingInstruction` (JS-объект без `__nid__`).
4. Исключение из фильтра не пробрасывается (`Propagate exception from filter function`,
   `Recursive filters need to throw`) — `_nf_accepts` глотает его в `try/catch` и отвечает
   `FILTER_REJECT`.
5. `toString()` — не `[object TreeWalker]`/`[object NodeIterator]`; атрибуты не readonly.

## Что чинить

(1) — добавить константы; (4) — пробрасывать исключение и выставлять флаг «active» по DOM §6.1;
(2)/(3) — после BUG-1161 обход через общий `parentNode`/`firstChild`, а не через арену.
