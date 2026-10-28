# BUG-1781 — `fetch(blob:…)` внутри Worker: `unsupported scheme: blob`

**Статус:** OPEN
**Заведён:** 2026-10-28 (P2, SITES-TRIAGE-2)
**Область:** js / network (`crates/js/src/worker_shims.rs`, `crates/network/src/lib.rs:512`)

## Источник: сайты

b2b-center-ru (вес 2, `text_len` 1 при 8808 у Chrome), uchi-ru (вес 2, базовый прогон; не в срезе из 30) — **2 сайта, вес кластера 4**. Отчёт: [`docs/perf/runs/2026-10-20-compat-triage.md`](../docs/perf/runs/2026-10-20-compat-triage.md) §Разбор blank. Прогон — `compat_crawl.py` живым окном, `LUMEN_NO_ADBLOCK=1`, без прокси, 2026-10-28.

## Симптом

stderr: `[worker] fetch: blob:https://b2b-center.ru/<uuid>: network error: unsupported scheme: blob`. Воркер, которому страница передала `blob:`-адрес, получает отказ сети — приложение остаётся с пустым телом (b2b-center: `ready` 4,7 с, текст 1).

## Минимальная репродукция

Страница делает `const u = URL.createObjectURL(new Blob(['hello']))`, создаёт `Worker` из `blob:`-скрипта, шлёт ему `u`; воркер выполняет `fetch(u).then(r => r.text())` и отвечает `postMessage`. Ожидание: `hello`. Тот же `fetch` в главном потоке работает (проверено `--dump-layout`: `main:hello`); в воркере отказ виден по живому журналу. Headless-дамп не дожидается ответа воркера, поэтому тест в `crates/js` должен ждать `postMessage`.

## Ожидание (Chrome)

Воркер резолвит `blob:`-URL из хранилища объектных URL своего origin (File API, Blob URL Store) так же, как главный поток.

## Где искать

Путь `fetch` воркера (`worker_shims.rs`, журнал `[worker] fetch:`) уходит прямо в сеть; перед ним нужна ветка `blob:` на хранилище объектных URL документа-владельца.

## Как проверить

Тест на сценарий выше; b2b-center.ru и uchi.ru в живом окне не пишут `unsupported scheme: blob`.
