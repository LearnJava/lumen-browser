# BUG-765 — ни один `[SecureContext]`-API не гейтится по `window.isSecureContext`: флаг вычислен, но не читается никем

**Статус:** FIXED 2026-09-07 (P3)
**Компонент:** js (`crates/js/src/dom.rs` — `WEB_API_SHIM`: `window.crypto.subtle`
(`dom.rs:12176`), `navigator.clipboard`, `navigator.serviceWorker`,
`navigator.geolocation`; `crates/js/src/*.rs` — шимы сенсоров, wake lock и др.)
**Найден:** P3, при закрытии [BUG-399](BUG-399-FIXED.md), 2026-08-11

## Симптом

`grep -rn "isSecureContext" crates/` вне блока определения самого свойства
(`dom.rs`) не даёт ни одного попадания: значение вычисляется (после
[BUG-399](BUG-399-FIXED.md) — верно, по W3C Secure Contexts §3.1/§3.2), отдаётся
странице и на этом заканчивается. Ни один API, помеченный в WebIDL
`[SecureContext]`, при установке шима не смотрит на контекст:

* `crypto.subtle` — весь `SubtleCrypto` доступен с обычной `http://`-страницы;
* `navigator.clipboard`, `navigator.serviceWorker`,
  `navigator.geolocation`, `navigator.wakeLock`;
* вся Generic Sensor family (`Accelerometer`/`Gyroscope`/`Magnetometer`/
  `*OrientationSensor`) — у неё нет и собственного гейта
  ([BUG-394](BUG-394-FIXED.md) закрыл наследование `EventTarget`, не контекст).

## Причина

Спека требует, чтобы `[SecureContext]`-интерфейс **отсутствовал** в глобале
небезопасного контекста (`'Gyroscope' in window === false`), а не бросал при
вызове. В шиме же все поверхности заводятся безусловно, одним прямым
присваиванием в `window`/`navigator`, без ветки по контексту. Пока
`isSecureContext` был литералом `true`, такой ветке было и неоткуда взяться —
условие всегда истинно; после BUG-399 источник истины появился, но потребителя
у него нет.

## Последствия

Наблюдаемая поверхность движка на небезопасном origin шире, чем у любого
реального браузера. Прямые следствия:

* `Gyroscope_insecure_context.html` (и близнецы в `accelerometer`,
  `magnetometer`, `orientation-sensor`) — `assert_false('Gyroscope' in window)`
  падает, даже если прогнать его не с loopback-адреса. Именно этот тест привёл
  к BUG-399, но одного верного флага для его позеленения недостаточно.
* Ранее зафиксированная и не заведённая находка того же класса —
  `docs/wpt-status.md`, категория `WebCryptoAPI` (2026-07-22): «`crypto.subtle`/
  `SubtleCrypto`/`CryptoKey` доступны из небезопасного контекста — secure-context
  gate для Web Crypto отсутствует вовсе». Настоящая заявка её и оформляет.
* Fingerprint-вектор: набор доступных API не зависит от схемы страницы, что
  само по себе отличает Lumen от браузеров, на которые он мимикрирует.

## Исправление

Заведён один общий предикат — `_lumen_secure_context` (`web_api_shim_mid_b.js`,
верхняя часть файла), вычисляемый один раз через уже существующий
`_lumen_url_is_potentially_trustworthy` (BUG-399) и переиспользуемый и самим
геттером `window.isSecureContext` (`web_api_shim_tail_mc.js`, раньше
пересчитывал то же самое второй раз), и всеми гейтами ниже. Условие везде
одно: `_lumen_secure_context !== false` — `undefined` (сборка без
`WEB_API_SHIM`, юнит-тест шима в изоляции) читается как «секьюрно», иначе
каждый файл `crates/js/src/*.rs`, ставящий свой шим отдельно от `dom.rs` в
собственных тестах, увидел бы поверхность пропавшей не из-за контекста, а
из-за отсутствия переменной.

Снято по WebIDL, не по вызову — свойство/интерфейс не заводится вовсе,
`'X' in window/navigator` даёт `false`, а не throw:

* `navigator.serviceWorker`, `navigator.clipboard` — `web_api_shim_mid_b.js`;
* `window.crypto.subtle`, `window.CryptoKey` (не весь `Crypto` — `getRandomValues`/
  `randomUUID` не помечены `[SecureContext]`) — `web_api_shim_tail_b.js`;
* `navigator.wakeLock` — оба места, где он заводится: Phase-0 стаб
  (`web_api_shim_tail_b.js`) и настоящий модуль `wake_lock.rs`, который
  переопределяет `navigator.wakeLock` позже в `install_dom`;
* вся Generic Sensor family (`Sensor` и подклассы) — один ранний `return` в
  начале `GENERIC_SENSOR_SHIM` (`generic_sensor.rs`), до объявления любого
  класса.

`navigator.geolocation` — особый случай: сам интерфейс не `[SecureContext]`
(реальные браузеры держат его на любом origin), но по спеке и вендоренному
`non-secure-contexts.http.html` оба входа (`getCurrentPosition`/
`watchPosition`) обязаны асинхронно резолвить `PERMISSION_DENIED` на
небезопасном origin вне зависимости от сконфигурированных `FakeCoords`
(`geolocation.rs`).

6 новых юнит-тестов (по одному на `absent_on_insecure_origin` в
`v8_events_cache.rs`/`v8_fullscreen_locks.rs`/`v8_generic_sensor.rs`/
`v8_idle_message_clipboard.rs`/`v8_webcrypto.rs`, плюс уже существовавший
`is_secure_context_is_false_on_insecure_origin`), остальные тесты этих пяти
файлов переведены на явный секьюрный/несекьюрный `https://`/`http://` URL
вместо прежнего пустого (по BUG-399 — небезопасного) URL фикстуры, иначе они
стали бы падать на пропавшем свойстве вместо проверки его поведения.
`cargo test -p lumen-js --features v8-backend` — 3535/3536 зелёных (единственный
провал, `native_binding_panic_does_not_abort_process`, воспроизводится и на
немодифицированном дереве — не регрессия этого фикса). `cargo clippy -p
lumen-js --all-targets --features v8-backend` чист; workspace-clippy целиком
не прогнать на этой машине — локальный rustc/clippy 1.98.1 против пина 1.97.0
красит несвязанные файлы (`chunks_exact` в `lumen-image`/`lumen-font`),
подтверждено `git diff --stat`.

## Связанные

* [BUG-399](BUG-399-FIXED.md) — источник истины (`window.isSecureContext`);
  эта заявка — прямой остаток от его закрытия.
* [BUG-766](BUG-766-OPEN.md) — в `WorkerGlobalScope` самого флага нет, так что
  гейт в воркере будет нечем питать.
* [BUG-669](BUG-669-OPEN.md) — `wakelock-insecure-context.any.html` формально
  PASS, но по неверной причине (`WakeLock` не выставлен вовсе): после гейта
  причина станет верной, тест — по-прежнему зелёным.
* [BUG-682](BUG-682-OPEN.md), [BUG-709](BUG-709-OPEN.md) — категории, чей
  сигнал сейчас частично съеден отсутствием гейта.
