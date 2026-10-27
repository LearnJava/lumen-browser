# BUG-1756 — Глобалы воркеров: нет `atob`/`btoa` в SharedWorker, нет `URL.createObjectURL` в Worker

**Статус:** OPEN
**Заведён:** 2026-10-27 (P2, WPT-FETCH-SCOUT, кластер W в [`docs/wpt-vendor-notes/fetch.md`](../docs/wpt-vendor-notes/fetch.md))
**Область:** js (`crates/js/src/shared_worker.rs` — установка глобальной области SharedWorker; `crates/js/src/worker.rs`/`worker_shims.rs` — `URL` в DedicatedWorker)

## Симптом

- `*.any.sharedworker.html` — `ReferenceError: btoa is not defined`: 72 сабтеста `http-cache/*` (хелпер `http-cache/http-cache.js` кодирует ключ через `btoa`) и harness ERROR у 5 id `api/credentials/authentication-*.any.sharedworker.html`.
- `api/basic/scheme-blob.sub.any.worker.html` — harness ERROR `URL.createObjectURL is not a function` (27 сабтестов того же файла в окне проходят).

`WindowOrWorkerGlobalScope` (HTML §8.3) обязывает `atob`/`btoa` в любом воркере; `URL.createObjectURL` — File API §8 — доступен в Window и DedicatedWorker/SharedWorker.

## Причина

Документирующий комментарий `install_shared_worker_global` ([shared_worker.rs:1077](../crates/js/src/shared_worker.rs#L1077)) прямо говорит, что бросающие `atob`/`btoa` выделенного воркера (`worker.rs`) сюда не перенесены. `URL.createObjectURL` в шиме выделенного воркера не установлен (блоб-реестр есть только у окна).

## Что делать

1. Поставить в SharedWorker те же `atob`/`btoa`, что у выделенного воркера (общий хелпер, не копия).
2. `URL.createObjectURL`/`revokeObjectURL` в DedicatedWorker и SharedWorker — через тот же блоб-реестр процесса, что и в окне (URL с origin воркера).

## Как проверить

- `run_smoke.py /fetch/http-cache/status.any.sharedworker.html /fetch/api/credentials/authentication-basic.any.sharedworker.html /fetch/api/basic/scheme-blob.sub.any.worker.html` — без `is not defined`/`is not a function`.
- Ожидаемый прирост `fetch` ≈ +10 баллов; тот же пробел бьёт другие категории с `.any.sharedworker`.
