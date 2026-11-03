# BUG-1782 — У `console` нет `assert`, `time`/`timeEnd`/`timeLog`, `count`, `group*`, `table`, `dir`, `trace`, `clear`

**Статус:** OPEN
**Заведён:** 2026-10-28 (P2, SITES-TRIAGE-2)
**Область:** js (`crates/js/src/shim/web_api_shim_mid.js:11646`, `var console`)

## Источник: сайты

claude-ai (вес 2, `text_len` 2 при 3678 у Chrome; `Uncaught TypeError: console.assert is not a function` в `shared-l-p-m-6-wjVx6_wQ.js`), kaspersky-ru (вес 2, базовый прогон: `console.time is not a function`) — **2 сайта, вес кластера 4**. Отчёт: [`docs/perf/runs/2026-10-20-compat-triage.md`](../docs/perf/runs/2026-10-20-compat-triage.md) §Разбор blank. Прогон — `compat_crawl.py` живым окном, `LUMEN_NO_ADBLOCK=1`, без прокси, 2026-10-28.

## Причина (подтверждена)

Объект `console` в shim содержит только `log/warn/error/info/debug`. Проверка `typeof console[k]` по списку методов Console Standard в `--dump-layout` даёт отсутствующими: `assert, clear, count, countReset, dir, dirxml, group, groupCollapsed, groupEnd, table, time, timeEnd, timeLog, trace`.

## Ожидание (Chrome)

Все методы Console Standard — функции; `assert` при ложном условии пишет в `error`, `time*`/`count*` ведут счётчики, `group*` — отступ, остальные — как минимум no-op.

## Минимальная репродукция

```html
<pre id=o></pre><script>o.textContent = ['assert','time','timeEnd','count','group','groupEnd','table','dir','trace','clear'].filter(function(k){return typeof console[k]!=='function'}).join();</script>
```

Должно напечатать пустую строку.

## Как проверить

Репродукция печатает пусто; claude.ai и kaspersky.ru в живом окне не пишут `is not a function` про `console.*`. Те же методы нужны в консолях воркеров (`worker_shims.rs:356`, `shared_worker.rs:339`, `sw_worker.rs:425`).
