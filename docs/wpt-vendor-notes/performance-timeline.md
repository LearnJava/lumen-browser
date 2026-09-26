# WPT vendor notes — `performance-timeline`

## Прогон и находки (`docs/wpt-status.md`)

Вендорена целиком 2026-08-05 (коммит `35be3b44`, `tests/wpt/performance-timeline/`, 70 файлов; `LICENSE-WPT.md` скопирован из соседней `navigation-timing`). `run_report.py --all --root performance-timeline --recursive` (~4.5 мин, 51 отобранный id): **36/51 harness OK, 19/69 сабтестов** — заметно выше нормы для этого бэклога, `PerformanceObserver` реально реализован и подключён. Доминирующая находка: `observe()` не валидирует форму аргумента вовсе (нет `TypeError` на `observe({})`, нет `InvalidModificationError` при смешивании `type`/`entryTypes` между вызовами — `po-observe-type.any.html` 4/5 сабтестов FAIL) и доставка колбэков синхронна (все `_lumen_deliver_*`/`mark`/`measure` зовут `_perf_observer_notify` напрямую, не через `queueMicrotask`) вместо очереди задач по спеке — `disconnect()` не успевает отменить уже вызванный колбэк (`po-disconnect.any.html`), `observe({buffered:true})` доставляет синхронно внутри самого `observe()` (`buffered-does-not-sync-invoke.html`), `takeRecords()` задваивает записи. Заведён [BUG-648](../../bugs/BUG-648-OPEN.md). Реконфирмации: `PerformanceObserverEntryList` не определён как глобал (класс [BUG-645](../../bugs/BUG-645-FIXED.md)/[BUG-624](../../bugs/BUG-624-FIXED.md)/[BUG-637](../../bugs/BUG-637-OPEN.md)/[BUG-589](../../bugs/BUG-589-FIXED.md)), относительные URL не резолвятся при fetch ([BUG-347](../../bugs/BUG-347-FIXED.md)), detached iframe без browsing context, `/common/dispatcher/dispatcher.js` не вендорен. Побочно (не расследовано): `window.stop is not a function`

## BUG-648 закрыт (2026-09-26, P6)

`observe()` валидирует аргументы, доставка идёт через задачу §5.3, `takeRecords()` опустошает
буфер наблюдателя, `PerformanceObserverEntryList` стал глобальным интерфейсом. Замер категории
после фикса — в [BUG-648](../../bugs/BUG-648-FIXED.md). `window.stop` вынесен в
[BUG-1186](../../bugs/BUG-1186-OPEN.md). Baseline `tests/wpt/metadata/performance-timeline/`
переснят: +28 PASS. `idlharness.any.html` теперь доходит до проверок интерфейсов, у
`PerformanceEntry` их 13, и все падают ([BUG-1189](../../bugs/BUG-1189-OPEN.md)).
`not-clonable.html` висит из-за `window.top` во фрейме ([BUG-1187](../../bugs/BUG-1187-OPEN.md)).
