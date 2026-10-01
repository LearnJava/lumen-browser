# BUG-1160 — `Range-mutations-*`: одна страница исчерпывает лимит арены (`DOM node limit exceeded`), три файла виснут

**Статус:** FIXED 2026-10-01 (P3)
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

## Починка (P3, 2026-10-01)

Диагноз подтверждён пробой: скрипт в одной синхронной задаче (`for` с 120 000 `createElement`) упирался
в `QuotaExceededError` на 49 992-м узле при нуле живых узлов. Тик GC оболочки (30 с) внутри такого цикла
не получает хода: `WeakRef`-цели закреплены до конца задачи (`KeepDuringJob`), а `FinalizationRegistry`
отпускает `js_refs` отдельной platform-задачей. Две причины, обе закрыты:

1. **Синхронный реклейм по требованию.** Нативная `_lumen_dom_reclaim_now`
   (`install_dom_reclaim`, `crates/js/src/v8_runtime/install/dom_core.rs`): `clear_kept_objects` →
   `low_memory_notification` ×2 → прокачка platform-очереди (колбэки финализатора зовут
   `_lumen_dom_release_ref`, поэтому мьютекс документа на это время не держится) →
   `dead_node_ids` + `reclaim_dead_nodes`. Шим (`web_api_shim_mid.js`, `_lumen_reclaim_and_retry`) оборачивает
   шесть `_lumen_create_*`: при `-1` один раз зовёт реклейм, пропускает освобождённые id через
   `_lumen_gc_collect` (слот не наследует слушатели/состояние прежнего узла) и повторяет создание.
   Пустой проход гасит следующий на 500 мс — страница с реально 50 000 живых узлов не платит полным GC
   за каждый неудавшийся `createElement`.
2. **Лимит считал длину арены, а не занятые слоты.** `try_create_*` сравнивали `nodes.len()` с
   `MAX_DOM_NODES`, хотя освобождённые слоты уже переиспользуются (`alloc`). Теперь
   `Document::live_node_count()` = `nodes.len() − free_slots.len()`.

Замер: проба на 120 000 узлов — 49 992 + `QuotaExceededError` до, 120 000 без ошибки за 1,5 с после.
`dom/ranges`: `DOM node limit exceeded` — 0 вхождений в логе (было 1699 + 37).
`Range-mutations-dataChange` теперь доходит до конца (468/2808, harness OK, раньше ERROR 209/2808).

Гипотеза про TIMEOUT `Range-mutations-appendChild`/`-insertBefore`/`-replaceChild` **не подтвердилась**:
после починки лимита они виснут так же (EXTERNAL-TIMEOUT на 60 с), `DOM node limit` в их логе нет.
Эти три — симптом [BUG-1232](BUG-1232-OPEN.md) (live ranges), как и остаток `dataChange`/`replaceData`.
`dom/traversal/NodeIterator-removal.html` (TIMEOUT) в этом срезе не перепроверялся — отдельная причина,
если останется.

Тесты: `dom_create_element_reclaims_detached_garbage_at_limit`,
`dom_create_element_still_throws_when_limit_is_all_live` (`v8_perf_typedom_node.rs`),
`try_create_element_ok_after_reclaim_frees_slots_at_limit` (`lumen-dom`). Три прежних теста-предзаполнения
арены теперь крепят узлы к корню: отсоединённый мусор реклеймится по требованию.

