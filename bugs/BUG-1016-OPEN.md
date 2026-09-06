# BUG-1016 — `atob`/`btoa` бросают `TypeError` вместо `DOMException InvalidCharacterError`

**Статус:** OPEN
**Заведён:** 2026-09-06 (P2, WPT-RUN-7 срез 17 — `html/webappapis`)
**Область:** js (`crates/js/src/worker.rs::atob_native_v8`/`btoa_native_v8`, зарегистрированы
и для главного окна через `crates/js/src/v8_runtime.rs:922` → `install_worker_bindings_v8`,
и для воркеров; тот же код продублирован в `crates/js/src/sw_worker.rs`)

## Симптом

`html/webappapis/atob/base64.any.html` (и `.any.worker.html` — тот же тест, второй прогон
в воркере) — 308 упавших ассертов (164 `atob`, 144 `btoa`) из общего числа FAIL прогона:

```
FAIL btoa("עברית") must raise INVALID_CHARACTER_ERR - assert_throws_dom: ... threw object
"TypeError: btoa: character out of Latin1 range" that is not a DOMException
InvalidCharacterError: property "code" is equal to undefined, expected 5

FAIL atob("a") - assert_throws_dom: ... threw object "TypeError: atob: invalid base64
string" that is not a DOMException InvalidCharacterError: property "code" is equal to
undefined, expected 5
```

## Причина

`atob_native_v8`/`btoa_native_v8` (`worker.rs:2010-2048`) при невалидном входе (base64
не декодируется, либо строка содержит символ вне Latin-1 для `btoa`) зовут
`throw_type_error` — обычный JS `TypeError`. По спеке (HTML LS
[§8.3 `WindowOrWorkerGlobalScope.atob/btoa`](https://html.spec.whatwg.org/multipage/webappapis.html#dom-windowbase64-atob))
оба метода обязаны бросать `DOMException` с именем `InvalidCharacterError`, а не
`TypeError`. Комментарий в коде («matching the QuickJS `atob` native's
`Err(rquickjs::Error::Exception)`») — след миграции с `rquickjs` на V8 (`CLAUDE.md`:
rquickjs давно выведен из workspace), решение просто не пересмотрено при переезде.

Тот же дефект дублирован в `crates/js/src/sw_worker.rs` (service worker — регистрирует
`"atob"`/`"btoa"` отдельно, не переиспользуя `worker.rs`'s natives).

## Почему это важно

Не косметика: любой код, ловящий `DOMException`/`e.name === 'InvalidCharacterError'`
вокруг `atob`/`btoa` (стандартный паттерн для валидации пользовательского base64),
получит необработанное исключение неправильного типа. 308 упавших ассертов на один
тест-файл (запущенный дважды — окно и воркер) — самый плотный кластер FAIL всего среза.

## Возможный путь фикса

Остальной JS-код проекта конструирует `DOMException` через `eval`-нутый JS-шим
(`new DOMException(msg, 'InvalidCharacterError')`, см. `broadcast_channel.rs:213`,
`web_api_shim_mid.js:9606`), а не бросает нативный V8-exception из Rust — нет общего
Rust-хелпера `throw_dom_exception`. Вероятно самый дешёвый путь: понизить
`atob_native_v8`/`btoa_native_v8` до сырых примитивов (`_lumen_atob_raw`/`_lumen_btoa_raw`,
возвращают `null`/бросают только на действительно неожиданный вход), а `atob`/`btoa`
как публичные глобалы объявить в JS-шиме, оборачивая вызов проверкой и
`throw new DOMException(..., 'InvalidCharacterError')` — тем же паттерном, что
`createProcessingInstruction`. Требует правки в трёх местах (`worker.rs`, `sw_worker.rs`
и месте, откуда `install_worker_bindings_v8` зовётся для окна) — проверить, не разошлись
ли уже `worker.rs`/`sw_worker.rs` копии сообщением об ошибке.

## Воспроизведение

```
LUMEN_PROFILE=dev-release tests/wpt/.venv/bin/python3 tests/wpt/run_report.py \
  --binary target/dev-release/lumen html/webappapis/atob/base64.any.html --all \
  --root html/webappapis --recursive --limit 1
```
или напрямую `run_smoke.py html/webappapis/atob/base64.any.html`.

## Не проверялось

Остальные `WindowOrWorkerGlobalScope` методы, теоретически подверженные тому же классу
(перенос спекового `DOMException` на голый `TypeError` при миграции с rquickjs) — не
искалось системно, взят только этот тест-файл.
