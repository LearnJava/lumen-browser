# BUG-361 — `document.permissionsPolicy.features()` возвращает ключи объявленной политики вместо списка фич, поддерживаемых движком (на обычной странице — всегда `[]`)

**Статус:** OPEN
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
* `encrypted-media` — этот тест.

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

## Возможный фикс (не реализован в этой сессии)

1. Развести две таблицы. Добавить в шим статический список поддерживаемых
   движком фич — `_ppSupported` — и вернуть из `features()` именно его
   (объединённым с ключами `_ppStore`, как того требует §8.2: реестр UA
   плюс всё, что реально объявлено).
2. Наполнить `_ppSupported` честно, а не всем реестром: перечислять только те
   имена, для которых у Lumen есть хоть какая-то реализация или осмысленный
   отказ. Иначе получится BUG-354 наоборот — реклама несуществующего.
   Первые кандидаты — то, что в движке есть: `fullscreen`, `geolocation`
   (сверить с `CAPABILITIES.md` перед фиксацией списка).
3. `encrypted-media` в этот список **не** класть: EME в Lumen не реализован
   вовсе (проба: `navigator.requestMediaKeySystemAccess`, `MediaKeys`,
   `MediaSource` — все `undefined`), так что тест
   `encrypted-media-supported-by-permissions-policy.tentative.html` и после
   фикса обязан падать — и это будет правильный, честный провал.
4. Заодно проверить `allowedFeatures()` (`permissions_policy.rs:52`): он
   фильтрует `Object.keys(_ppStore)` и потому на странице без заголовка тоже
   вернёт `[]`. По спеке это «фичи, разрешённые текущему origin» — при
   default-allow там должен оказаться весь `_ppSupported`.

Не чинилось в этой сессии — P2-wpt вендорит и обследует, кодовые фиксы — полоса
P3 (`CLAUDE.md`, developer assignments).
