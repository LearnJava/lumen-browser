# BUG-1016 — `atob`/`btoa` бросают `TypeError` вместо `DOMException InvalidCharacterError`

**Статус:** FIXED 2026-09-21 (P3)
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

## Фикс (2026-09-21, P3)

Реализован путь из §Возможный путь фикса для window и dedicated worker (два места,
покрытые тест-файлом): `worker.rs`'s `atob_native_v8`/`btoa_native_v8` понижены до
сырых примитивов, зарегистрированных под `_lumen_{atob,btoa}_impl` (возвращают
`undefined` на ошибке, ничего не бросают сами — конструировать `DOMException` из
native V8-кода нечем, см. §Возможный путь фикса), новый JS-шим
`WORKER_ATOB_BTOA_SHIM` объявляет публичные `atob`/`btoa`, вызывает примитив и
бросает `new DOMException(msg, 'InvalidCharacterError')` на `undefined`; worker-скоуп
получил `DOM_EXCEPTION_POLYFILL` (которого там раньше не было вовсе, см. комментарий
у `structuredClone`, GAP-WORKERSCOPE срез 2). Окно (`web_api_shim_mid_c.js`) — уже
чистый JS с собственным b64-алгоритмом, менять пришлось только `throw new TypeError`
→ `throw new DOMException(..., 'InvalidCharacterError')`, без нового native-кода.
Тесты: `worker::tests_v8::v8_worker_atob_btoa_throw_dom_exception`,
обновлён `v8_atob_throws_on_invalid_input`; для окна —
`dom::tests::v8_url_abort_clone_blob::{atob_invalid_input_throws_dom_exception,
btoa_out_of_latin1_throws_dom_exception}`.

**`sw_worker.rs` НЕ тронут — сознательное решение, не пропуск.** Дубль там
регистрирует `atob`/`btoa` без throw вовсе (просто `undefined`), и это не
случайность: `install_sw_globals_v8`'s собственный `caches.put()` (строка ~235)
зовёт глобальный `btoa(text)` на произвольном UTF-8-тексте сетевого ответа —
любой не-ASCII текст (кириллица, эмодзи, вообще что угодно не-Latin1) тривиально
встречается в реальных телах ответов. Применение того же `WORKER_ATOB_BTOA_SHIM`
здесь означало бы, что `caches.put()` начинает падать на каждом нелатинском теле
ответа — Cache API service worker'а сломался бы для обычного некэшируемого сейчас
контента. Симптом этого бага (308 упавших ассертов) целиком из `.any.html`/
`.any.worker.html` — окно и dedicated worker, ServiceWorker этим тест-файлом не
покрыт — так что фикс полностью закрывает измеренный симптом без этого риска.
Если `sw_worker.rs`'s `atob`/`btoa` когда-нибудь тоже нужно привести к спеке,
это отдельная задача: `caches.put()`'s использование `btoa` как небросающего
"encode arbitrary bytes" сначала нужно перевести на выделенный internal-примитив
(`_lumen_sw_b64_encode` или подобный), не завязанный на публичный спековый `btoa`.

## Не проверялось

Остальные `WindowOrWorkerGlobalScope` методы, теоретически подверженные тому же классу
(перенос спекового `DOMException` на голый `TypeError` при миграции с rquickjs) — не
искалось системно, взят только этот тест-файл.
