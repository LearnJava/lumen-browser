# BUG-1782 — У `console` нет `assert`, `time`/`timeEnd`/`timeLog`, `count`, `group*`, `table`, `dir`, `trace`, `clear`

**Статус:** FIXED 2026-11-14 (P3)
**Заведён:** 2026-10-28 (P2, SITES-TRIAGE-2)
**Область:** js (шим `shim/console_shim.js` — новый; `web_api_shim_mid.js`, `worker_shims.rs`, `shared_worker.rs`, `sw_worker.rs`, `dom.rs`)

## Источник: сайты

claude-ai (вес 2, `text_len` 2 при 3678 у Chrome; `Uncaught TypeError: console.assert is not a function` в `shared-l-p-m-6-wjVx6_wQ.js`), kaspersky-ru (вес 2, базовый прогон: `console.time is not a function`) — **2 сайта, вес кластера 4**. Отчёт: [`docs/perf/runs/2026-10-20-compat-triage.md`](../docs/perf/runs/2026-10-20-compat-triage.md) §Разбор blank. Прогон — `compat_crawl.py` живым окном, `LUMEN_NO_ADBLOCK=1`, без прокси, 2026-10-28. Тот же дефект убивал Rive WASM на ebay.com (`console.assert` перед загрузкой rive-васма — journal 2026-11-12, единственная JS-ошибка страницы).

## Диагноз

Объект `console` в шиме содержал только `log/warn/error/info/debug` (страница) и `log/info/warn/error/debug` (воркеры; в сервис-воркере — все no-op). Проверка `typeof console[k]` по списку методов Console Standard давала отсутствующими 14 методов: `assert, clear, count, countReset, dir, dirxml, group, groupCollapsed, groupEnd, table, time, timeEnd, timeLog, trace`. Четыре области исполняли четыре независимых литерала — расхождение поверхностей росло бы с каждым флейвором.

## Исправление

Одна фабрика вместо четырёх литералов: новый `shim/console_shim.js` определяет `_lumen_make_console(emit)` — полный набор методов Console Standard со своим состоянием (счётчики, таймеры, глубина групп) в замыкании на область. `emit(level, text)` — единственная точка расхождения: каждая область подставляет свой сток.

1. **Страница** (`web_api_shim_mid.js`): `var console = _lumen_make_console(...)` с теми же тремя нативами `_lumen_console_log/warn/error`, что и раньше (уровни `log`/`warn`/`error` маппятся как прежде; `debug` по-прежнему пишется в `log`).
2. **CONSOLE_SHIM** (`dom.rs`): константа сплайсится в `web_api_shim()` (перед MID, чей `var console` её вызывает) и в `worker_exposed_shim()` — по рабочему правилу WORKER-1 воркеры исполняют дословный кусок страничной программы (пин тестом `worker_exposed_shim_is_a_verbatim_slice_of_the_page_shim`).
3. **Dedicated/shared worker** (`worker_shims.rs`, `shared_worker.rs`): фабрика + прежний одиночный натив `_lumen_worker_console_log`/`_lumen_sw_console_log` с префиксами `[WARN] `/`[ERR]  ` (формат сохранён); `debug` теперь тоже пишется (как у страницы и Chrome).
4. **Service worker** (`sw_worker.rs`): фабрика с no-op стоком — методы существуют, вывод по-прежнему не идёт (в scope нет консольного стока, поведение не менялось).

Поведение по Console Standard: `assert` пишет в error только при ложном условии («Assertion failed: …»), `time`/`timeLog`/`timeEnd` — таймеры с меткой (дефолт `default`, отсутствие — warn «Timer … does not exist»), `count`/`countReset` — счётчики, `group`/`groupCollapsed`/`groupEnd` — отступ двумя пробелами на уровень, `trace` — «Trace: …» + стек `new Error().stack` в error, `clear` — no-op, `dir`/`dirxml`/`table` — как `log` (структурные принтеры — поверхность DevTools, для страницы неотличимо).

## Тесты

- `dom::tests::v8_core::v8_bug1782_console_standard_surface` — репро карточки (фильтр `typeof` по 14 методам пуст), `assert(true)` молчит и `assert(false, …)` пишет «Assertion failed: condition failed» в error, `timeEnd` логирует длительность, `count` дважды даёт `c: 1`/`c: 2`, группа отступает тело и `groupEnd` возвращает глубину.
- `shared_worker::tests_v8::shared_worker_console_has_standard_surface` — вся поверхность в shared-worker scope, стейтфульные пути идут через флейворный сток без краха.
- `web_api_shim_splices_its_parts_in_source_order` и `worker_exposed_shim_is_a_verbatim_slice_of_the_page_shim` — расширены пином фабрики в обеих сборках.

## Известные пределы

- `dir`/`dirxml`/`table` логируют строкой (Chrome в DevTools рисует интерактивные объекты) — наблюдаемого для страницы расхождения нет.
- Сервис-воркер молчит, как и прежде: стока в scope нет (это отдельный пробел, не часть бага).
- `timeLog`/`timeEnd` для несуществующего таймера warn-ят, как Chrome; таймеры не переживают `clear()` (как и в Chrome до DevTools-фильтра) — поведение не специфицировано строже.

## Как проверить

Репро карточки печатает пустую строку (тест `v8_bug1782_console_standard_surface` выше). Живая сверка (compat_crawl, живое окно, `LUMEN_NO_ADBLOCK=1`, corpus-ab, `.tmp/compat-crawl/20261114-073218`, коммит `d2ac5d555` + фикс): проверка exceptions — 0 новых сигнатур у обоих сайтов, `not a function` в логах 0; claude.ai — из blank (`text 2/3678`) в паритет по пикселям (diff 14.09 % при пороге 35 %), стены нет, у Chrome 1 исключение против 0 у Lumen (остаток — a11y 27/108, сбор имён — BUG-1793-семья, не этот баг); kaspersky-ru — `console.time is not a function` исчез, но страница HUNG/blank (движковый шторм, семейство BUG-1778 — отдельный дефект, до фикса она была DEGRADED blank с единственной ошибкой `console.time`).
