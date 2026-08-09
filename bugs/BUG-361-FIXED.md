# BUG-361 — `document.permissionsPolicy.features()` возвращает ключи объявленной политики вместо списка фич, поддерживаемых движком (на обычной странице — всегда `[]`)

**Статус:** FIXED 2026-08-09
**Компонент:** js (`crates/js/src/permissions_policy.rs:47-49` — `FeaturePolicy.prototype.features`; хранилище `_ppStore` заполняется только из HTTP-заголовка, `permissions_policy.rs:86+`)
**Найден:** P2, WPT-VENDOR-encrypted-media (2026-07-28), `run_report.py --all --root encrypted-media --recursive`, тест `encrypted-media-supported-by-permissions-policy.tentative.html`

## Симптом

`document.permissionsPolicy.features()` на любой странице без заголовка
`Permissions-Policy` возвращает пустой массив. Подтверждено вне WPT
`--dump-layout`-пробой (`.tmp/probe-em2.html`, обычная локальная страница):

```
P5 pp: [object Object] features: function() { …
P6 pp keys:                        // собственных ключей нет — всё на прототипе
```

и в самом WPT:

```
FAIL document.permissionsPolicy.features should advertise encrypted-media.
     - assert_in_array: value "encrypted-media" not in array []
```

Тест — трёхстрочный, вся его суть в `assert_in_array("encrypted-media",
document.permissionsPolicy.features())`.

## Причина

Шим держит одну-единственную таблицу `_ppStore` и использует её сразу в двух
несовместимых ролях:

```js
// crates/js/src/permissions_policy.rs:47
// Returns all feature names present in the active policy.
FeaturePolicy.prototype.features = function() {
  return Object.keys(_ppStore);
};
```

`_ppStore` наполняется **только** из `window._lumen_set_permissions_policy(headerValue)`
(`permissions_policy.rs:86+`), которую шелл зовёт с сырым значением заголовка
`Permissions-Policy`/`Feature-Policy`. Нет заголовка — таблица пуста — `features()`
отдаёт `[]`.

По спеке (W3C Permissions Policy §8.2, `FeaturePolicy.features()`) метод обязан
вернуть **«the set of feature names supported by the user agent»**, а не список
фич, перечисленных в политике документа. Для «что объявлено в политике» в том же
интерфейсе есть отдельные `allowedFeatures()` и `getAllowlistForFeature()` —
и они у Lumen как раз реализованы (`permissions_policy.rs:52`, `:59`). То есть
перепутаны именно роли: `features()` сейчас делает работу, близкую к
`allowedFeatures()`, а свою не делает вовсе.

Следствие: **feature detection через `features()` даёт ложноотрицательный
ответ** — зеркальная ошибка к BUG-354, где `PerformanceObserver.supportedEntryTypes`
рекламирует нереализованные типы записей и даёт ложноположительный.

## Масштаб

В `encrypted-media` — 1 сабтест из 6 (`encrypted-media-supported-by-permissions-policy.tentative.html`,
0/1). Но это уже **третья категория бэклога, упирающаяся в ту же строку**, и в
предыдущих двух причина была записана как «`features` не перечисляет фичу», без
root cause:

* `accelerometer` — `accelerometer-supported-by-permissions-policy.html`,
* `ambient-light` — `AmbientLightSensor-supported-by-permissions-policy.html`,
* `encrypted-media` — этот тест,
* `font-access` — `permissions-policy/local-fonts-supported-by-permissions-policy.html`
  (P2, WPT-VENDOR-font-access 2026-07-28): четвёртая категория, и здесь цена
  максимальна — это **единственный** тест категории, который вообще исполняется
  (остальные 11 уходят в HTTPS-порт-гэп и testdriver-SKIP), поэтому баг в
  одиночку определяет её результат: 1/12 harness OK, 0/1 сабтестов.
  Проба той же страницы: `features()` — 0 элементов, `allowsFeature('local-fonts')`
  — `true`, `allowsFeature('made-up-xyz')` — тоже `true` (для
  **нераспознанного** имени спека требует `false`, так что default-allow из
  `permissions_policy.rs:39` покрывает не только незаявленные, но и
  несуществующие фичи — учесть при фиксе п.1).

Апстрим содержит такой `*-supported-by-permissions-policy*` тест почти в каждой
категории, чья фича упомянута в реестре Permissions Policy (`gyroscope`,
`magnetometer`, `camera`, `microphone`, `geolocation`, `fullscreen`,
`payment`, `usb`, `midi`, …) — то есть по мере вендоринга бэклога этот же
сабтест будет падать ещё десятки раз.

Вне WPT: библиотеки, которые перед использованием API спрашивают
`document.featurePolicy.features().includes('camera')`, получат «не
поддерживается» на любой странице без заголовка и уйдут в fallback-ветку.
Обратной, более опасной ошибки здесь нет — `allowsFeature()` остаётся
default-allow (`permissions_policy.rs:38`), так что ничего лишнего не
разрешается.

## Фикс (P3, 2026-08-09)

Реализован ровно план из предыдущей сессии, п.1-2-3-4:

1. Добавлена отдельная константа `_ppSupported` — статический список имён
   фич, для которых у Lumen есть настоящая нативная реализация (не
   Phase-0-заглушка, всегда бросающая `NotSupportedError`). `features()`
   теперь возвращает объединение `_ppSupported` и `Object.keys(_ppStore)`
   (§8.2: реестр UA плюс всё, что реально объявлено в политике документа) —
   `permissions_policy.rs:47-56`.
2. Список `_ppSupported` собран консервативно, сверкой с реальным кодом
   (не с `CAPABILITIES.md` — там формулировки по подсистемам, не по именам
   реестра Permissions Policy): `fullscreen` (`dom.rs::requestFullscreen`,
   полная реализация с событиями), `geolocation` (`geolocation.rs` —
   настоящий API, по умолчанию `PERMISSION_DENIED`, как в браузере без
   выданного разрешения — это не заглушка, а честное поведение),
   `microphone` (`media_capture.rs` — реальный захват аудио через `cpal`),
   `screen-wake-lock` (`wake_lock.rs` — реальная блокировка сна),
   `display-capture` (`screen_capture.rs` — реальный захват экрана).
   Явно **не** включены: `camera` (только видео остаётся
   `NotSupportedError`, см. `media_devices.rs:203` — аудио отдельно от
   видео), `payment`/`midi`/`usb`/`hid`/`xr-spatial-tracking` (все
   Phase-0-заглушки, `throw ... NotSupportedError` безусловно) и
   сенсоры (`accelerometer`/`ambient-light-sensor`/`gyroscope`/
   `magnetometer` — не реализованы вовсе).
3. `encrypted-media` в список не включён — EME в Lumen действительно не
   реализован, `encrypted-media-supported-by-permissions-policy.tentative.html`
   и после фикса корректно проваливается (честный, ожидаемый результат).
4. `allowedFeatures()` переписан на `features().filter(allowsFeature)`
   вместо `Object.keys(_ppStore).filter(...)` — на странице без заголовка
   политики теперь возвращает весь `_ppSupported` (default-allow), а не `[]`
   (`permissions_policy.rs:58-63`).

6 новых юнит-тестов в `permissions_policy.rs` (без заголовка политики —
`_ppSupported` присутствует и не содержит `encrypted-media`/`camera`/
`payment`; `allowedFeatures()` по умолчанию включает поддерживаемые фичи;
явный запрет через политику убирает фичу из `allowedFeatures()`, но не из
`features()`). `cargo test -p lumen-js --features v8-backend
permissions_policy` — 10/10 OK. `cargo clippy -p lumen-js --all-targets
--features v8-backend -- -D warnings` — чисто.

Вне скоупа: `getAllowlistForFeature()` не трогался (уже корректен по
спеке); реальное серверное принуждение политики (enforcement) остаётся
Phase 1 задачей, как и было отмечено в шапке файла до фикса.
