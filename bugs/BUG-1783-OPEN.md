# BUG-1783 — ea.com: окно умирает — `bad_optional_access was thrown in -fno-exceptions mode`

**Статус:** OPEN
**Заведён:** 2026-10-28 (P2, SITES-TRIAGE-2)
**Область:** js (V8 / `crates/js`, сборка с `-fno-exceptions`) — не локализовано

## Источник: сайты

ea-com (вес 2, статус DEAD оба раза, `text_len` 0 при 7986 у Chrome) — **1 сайт, вес кластера 2**; воспроизведено 2 из 2 запусков (базовый прогон и повтор с `--restart-every 1`). Отчёт: [`docs/perf/runs/2026-10-20-compat-triage.md`](../docs/perf/runs/2026-10-20-compat-triage.md) §Разбор blank. Прогон — `compat_crawl.py` живым окном, `LUMEN_NO_ADBLOCK=1`, без прокси, 2026-10-28.

## Симптом

Живой журнал (`live.stderr.15.log`): картинки drop-assets.ea.com грузятся, затем `[JS error] Uncaught TypeError: Cannot read properties of undefined (reading 'call')` (4 раза, webpack-рантайм), `window.parent.handleToken is not a function` (auth-iframe `static-pages/auth.html#error=login_required`), `Battery Status API is not supported` и последней строкой — `bad_optional_access was thrown in -fno-exceptions mode`: V8 делает abort, окно мертво (`lumen_status` DEAD, `ready` не наступает).

## Что неизвестно

Какой вызов бросает `std::bad_optional_access`: пробой нашей привязки (чтение пустого `Option` в обёртке) или внутренняя ошибка V8 на конкретной странице. Первый шаг — минимизация: открыть `https://www.ea.com/` в живом окне, снять стек падения (`RUST_BACKTRACE=1` / WER-дамп), по нему сузить до скрипта.

## Как проверить

`python scripts/compat_crawl.py --only ea-com …` — статус не DEAD, окно живёт до конца таймаута.
