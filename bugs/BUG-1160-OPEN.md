# BUG-1160 — `Range-mutations-*`: одна страница исчерпывает лимит арены (`DOM node limit exceeded`), три файла виснут

**Статус:** OPEN
**Заведён:** 2026-09-25 (P6, при закрытии [BUG-863](BUG-863-FIXED.md))
**Область:** dom/js — `MAX_DOM_NODES = 50_000` (`crates/engine/dom/src/lib.rs:129`), сборка
отсоединённых узлов только по 30-секундному тику GC оболочки
(`crates/shell/src/app/about_to_wait.rs:1740`, `Document::reclaim_dead_nodes`); причина зависаний
не локализована

## Симптом

`tests/wpt/dom/ranges/Range-mutations.js` зовёт `setupRangeTests()` (≈50 узлов, из них часть в
отсоединённых документах) **перед каждым** `doTest` — сотни раз за страницу. Все старые узлы
становятся отсоединёнными, но арена их не переиспользует, пока не пройдёт GC-тик и V8 не соберёт
обёртки. Прогон `run_report.py --all --root dom/ranges --recursive --processes 4` (2026-09-25,
после BUG-863):

- `Range-mutations-dataChange.html` — ERROR, 209/2808, из них **1699 FAIL `DOM node limit exceeded`**
  и затем `Cannot read properties of null (reading 'style')`;
- `Range-mutations-replaceData.html` — ERROR, 845/1146, 37 FAIL `DOM node limit exceeded`;
- `Range-mutations-appendChild.html`, `-insertBefore.html`, `-replaceChild.html` — TIMEOUT
  «browser stopped answering automation» (перепроверено одиночным `run_smoke.py`: 1:04 до
  таймаута, в логе браузера после загрузки скриптов тишина). Связь зависания с лимитом арены
  **не проверена** — это гипотеза.

То же на `dom/traversal/NodeIterator-removal.html` — TIMEOUT.

## Почему это не только WPT

Страница, которая создаёт и выбрасывает узлы быстрее 50 000 за 30 с (виртуальные списки,
перерисовка шаблонов через `innerHTML`), упрётся в `QuotaExceededError`, хотя живых узлов у неё
мало. Лимит считает `nodes.len()`, а не живые узлы.

## Что проверить

1. Число живых/мёртвых узлов на момент первого `QuotaExceededError` в `dataChange`.
2. Что делает страница в зависших файлах (профиль/стек JS-потока).
