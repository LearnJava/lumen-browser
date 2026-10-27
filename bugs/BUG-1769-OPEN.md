# BUG-1769 — ошибки кросс-ориджин скрипта в `importScripts` не глушатся в `NetworkError`

**Статус:** OPEN
**Заведён:** 2026-10-27 (P2, WPT-HOST-WPT — на хосте `localhost` кросс-ориджин имя не резолвилось)
**Область:** js workers (`crates/js/src/worker_shims.rs` — `importScripts`, копия в `crates/js/src/shared_worker.rs`; `crates/js/src/worker.rs` — нативный `_lumen_import_script`)

## Симптом

`workers/interfaces/WorkerUtils/importScripts/catch.sub.any.worker.html`: скрипт с другого origin, падающий при компиляции или выполнении, должен дать в `importScripts` `DOMException` `NetworkError` без деталей исходной ошибки. Lumen пробрасывает исходную ошибку (`SyntaxError`/`Error` с текстом и стеком чужого скрипта).

HTML LS §10.3.1 «import scripts into worker global scope», шаг про «muted errors»: если скрипт классический и его ответ кросс-ориджин без CORS (muted errors), то при исключении в процессе выполнения бросается `"NetworkError"` `DOMException`. Сабтесты с same-origin скриптом ждут исходную ошибку — их не трогать.

## Причина

`importScripts` ([worker_shims.rs:371-391](../crates/js/src/worker_shims.rs#L371), та же копия — [shared_worker.rs:354](../crates/js/src/shared_worker.rs#L354)) берёт текст через `_lumen_import_scripts_resolve` и отдаёт его нативному `_lumen_import_script` ([worker.rs:430-439](../crates/js/src/worker.rs#L430)), который делает `Script::compile` без origin и без признака muted errors. Origin ответа (с учётом редиректов) до этого места не доходит.

## Что делать

1. `_lumen_import_scripts_resolve` должен отдавать вместе с текстом признак «ответ кросс-ориджин относительно воркера» (финальный URL после редиректов; `data:` — same-origin по правилам воркера, `blob:` — по origin blob-URL).
2. При muted errors: компилировать с `ScriptOrigin` `is_shared_cross_origin = false` и любое исключение компиляции/выполнения заменять на `DOMException("…", "NetworkError")`.
3. Поправить обе копии (dedicated и shared worker).

## Как проверить

- `run_smoke.py` на `/workers/interfaces/WorkerUtils/importScripts/catch.sub.any.worker.html` (и `.sharedworker.html`, если есть).
- `workers/interfaces/WorkerUtils/importScripts/*` — без регрессий.
