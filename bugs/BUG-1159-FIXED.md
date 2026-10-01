# BUG-1159 — `Range`: сравнение, проверки и операции над содержимым — заглушки

**Статус:** FIXED 2026-10-01 (P3)
**Заведён:** 2026-09-25 (P6, при закрытии [BUG-863](BUG-863-FIXED.md))
**Область:** js — `crates/js/src/shim/web_api_shim_mid.js`, `_lumen_make_range` (объектный литерал
Range, строки ~9917–10030)

## Симптом

Пока `setupRangeTests()` падал на `createCDATASection` (BUG-863), `dom/ranges` отдавал ERROR без
сабтестов. После починки категория впервые дошла до собственных утверждений:
`run_report.py --all --root dom/ranges --recursive --processes 4` — **46/57 harness OK,
12262/44063 сабтестов** (до — 33/57, 25/251).

Методы Range, которые в шиме стоят заглушками или считают не то:

| Метод | Что в коде | Сабтестов FAIL в прогоне |
|---|---|---:|
| `compareBoundaryPoints` | сравнивает **номера узлов арены** (`p[0] < p[2]`), а не порядок в дереве; не бросает `NotSupportedError` на `how` вне 0..3 и `WrongDocumentError` на разные корни | 9313 из 9313 |
| `comparePoint` | `return 0` | 5518 из 5580 |
| `isPointInRange` | `return false` | 1039 из 5733 |
| `intersectsNode` | `return false` | 248 из 2356 |
| `setStart`/`setEnd`/`set{Start,End}{Before,After}` | не проверяют `offset > length` (`IndexSizeError`), doctype (`InvalidNodeTypeError`), узел без родителя; при узле из другого дерева не схлопывают диапазон | 8071 из 10920 (`Range-set.html`) |
| `cloneContents` | `return null` | весь `Range-cloneContents.html` |
| `extractContents` | `deleteContents()` + `return null` | весь `Range-extractContents.html` |
| `surroundContents` | пустая функция | весь `Range-surroundContents.html` |
| `insertNode` | `appendChild` к **родителю** стартового контейнера | весь `Range-insertNode.html` |
| `cloneRange` | 0/62 — клон теряет прототип/поля (не разбирался) | 62 |

## Воспроизведение

`tests/wpt/dom/ranges/Range-comparePoint.html`, `Range-compareBoundaryPoints.html`,
`Range-set.html` — самодостаточны, iframe/testdriver не требуют.

## Что чинить

DOM §5.5: порядок граничных точек (tree order через общего предка), проверки из «set the start or
end», `comparePoint`/`isPointInRange`/`intersectsNode` по спековым шагам, clone/extract/surround/insert
по §5.5 «clone the contents»/«extract»/«insert».

## Исправлено

`_lumen_make_range` (`web_api_shim_mid.js`): порядок граничных точек по дереву (`_lumen_range_cmp`),
`compareBoundaryPoints` (проверки `how` как unsigned short, `WrongDocumentError`; константы
`END_TO_END=2`/`END_TO_START=3` по IDL и на самом `Range`), `comparePoint`/`isPointInRange`/
`intersectsNode`, проверки `set{Start,End}*`/`select*` (`IndexSizeError`, `InvalidNodeTypeError`,
схлопывание при другом корне), `cloneContents`/`extractContents`/`insertNode`/`surroundContents`
по DOM §5.5, `collapse()` без аргумента — в конец. `node_length` в `selection.rs` знает Comment/PI.
`dom/ranges`: 12262 → 31325 из 44063 сабтестов. Тесты — `bug1159_range_spec.rs`.

Остаток (не про сравнение/операции): отсоединённые/XML-документы и Attr без nid — [BUG-1161](BUG-1161-OPEN.md);
живое обновление диапазонов при мутациях DOM — [BUG-1232](BUG-1232-OPEN.md).

