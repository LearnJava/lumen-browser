# BUG-765 — ни один `[SecureContext]`-API не гейтится по `window.isSecureContext`: флаг вычислен, но не читается никем

**Статус:** OPEN
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

## Что нужно сделать

Ввести один общий предикат в шиме (значение того же замыкания, из которого
отдаётся `window.isSecureContext`) и снимать им поверхности при установке — не
бросать из методов, а не заводить свойство вовсе. Начать разумно с тех, у кого
есть вендоренные WPT-тесты на insecure context: Generic Sensor family, Web
Crypto, Clipboard, Service Workers. Не забыть, что `[SecureContext]` относится и
к интерфейсным объектам (`window.Gyroscope`), и к точкам входа на `navigator`.

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
