# BUG-399 — `window.isSecureContext` захардкожен `true` для любой страницы, не вычисляется из протокола/хоста

**Статус:** OPEN
**Компонент:** js (`crates/js/src/dom.rs:11531` — `window.isSecureContext = true;`;
доступный, но не использованный источник данных — `crates/js/src/dom.rs:7397`
`_lumen_loc_parts`, вычисленный из `_LUMEN_PAGE_URL`)
**Найден:** P2, WPT-VENDOR-gyroscope (2026-07-28), тест `Gyroscope_insecure_context.html`

## Симптом

Категория `gyroscope` (`tests/wpt/gyroscope/`, 15 файлов) даёт 2/10 harness OK
(остальное — SKIP по testdriver.js и TIMEOUT на HTTPS-порт-гэпе, обычный
профиль для этого backlog-а). Один из двух исполнившихся тестов,
`Gyroscope_insecure_context.html`, падает:

```
FAIL Gyroscope is not exposed in an insecure context.
  assert_false: Gyroscope must not be exposed expected false got true
```

Тест — буквально `assert_false('Gyroscope' in window)`
(`generic-sensor/generic-sensor-tests.js::runGenericSensorInsecureContext`).
Сам по себе этот конкретный прогон не решающий: тест исполнялся через
`wptserve` на `http://127.0.0.1:18300/…`, а loopback-адрес по W3C Secure
Contexts §3.1 — «potentially trustworthy origin», то есть спека и реальные
браузеры тоже дали бы здесь `isSecureContext === true` и тот же FAIL (тот же
класс несовпадения окружения с тестовым допущением, что уже задокументирован
для `audio-output`'s `secure-context.html`, `BUGS.md`/`VENDOR.md`, 2026-07-24) —
поэтому находка не в провале этого теста, а в чтении кода, к которому он
привёл.

## Причина

`window.isSecureContext` в `WEB_API_SHIM` — не вычисление, а буквальный
литерал:

```js
// W3C Secure Contexts §3.1: local-file and localhost are considered secure.
window.isSecureContext       = true;
```

Комментарий описывает намерение («local-file и localhost считаются secure»,
т.е. предполагается некоторая проверка), но код это не делает — присваивание
безусловное для любого протокола и хоста, включая настоящий
`http://example.com/` без loopback-исключения. К моменту этой строки шим уже
успел распарсить реальный URL страницы (`_lumen_loc_parts`, `dom.rs:7397`, из
`_LUMEN_PAGE_URL`, который Rust кладёт в контекст до эвала шима —
`dom.rs:322`) — protocol/hostname доступны, просто не читаются здесь.

Юнит-тест `is_secure_context_is_true` (`dom.rs:26088`) фиксирует текущее
поведение как ожидаемое («всегда true»), то есть это не регрессия, а
изначальное Phase 0/1 упрощение, никогда не доведённое до вычисления по
спеке.

## Последствия

Любой API, помеченный в WebIDL `[SecureContext]` (вся Generic Sensor family —
`Accelerometer`/`Gyroscope`/`Magnetometer`/`*OrientationSensor`, Web
Crypto `crypto.subtle`, Clipboard API, Service Workers и другие), в Lumen
никогда не гейтится по контексту — флаг, на который такой гейтинг обязан
полагаться, всегда отвечает «безопасно», даже на настоящей небезопасной
странице (`http://` не-loopback). Сейчас маскируется тем, что почти все
таких API и так проверяются по другим путям (`.https.`-порт-гэп на уровне
исполнителя WPT, не движка) или не реализуют собственный секьюрити-гейтинг
вовсе (генерик-сенсоры, см. соседний `BUG-394`) — то есть дефект не даёт
собственного видимого сигнала в большинстве категорий backlog-а, но
затрагивает инфраструктуру, общую для всех них.

## Что нужно сделать

Заменить литерал на вычисление по W3C Secure Contexts §3.1 (potentially
trustworthy origin): `protocol === 'https:' || protocol === 'wss:' ||
protocol === 'file:' || hostname === 'localhost' || hostname.endsWith
('.localhost') || <hostname — loopback IPv4/IPv6>`, используя уже
распарсенные `_lumen_loc_parts` вместо литерала `true`. Обновить
`is_secure_context_is_true` (`dom.rs:26088`) и добавить рядом
`is_secure_context_is_false_on_insecure_origin` — сегодня секьюрность
контекста не проверяется юнит-тестами вовсе, только «всегда true».

## Связанные

* Тот же корень уже наблюдался раньше и не оформлялся отдельным багом:
  `docs/wpt-status.md`, категория `WebCryptoAPI` (2026-07-22) — «`crypto.subtle`/
  `SubtleCrypto`/`CryptoKey` доступны из небезопасного (http, не secure
  context) контекста — secure-context gate для Web Crypto отсутствует вовсе»,
  помечено как «не заведён как BUG-NNN — первый проход» до того, как
  методология backlog-а стала требовать формальной заявки на каждую находку.
* `crates/js/src/dom.rs:7397` — уже посчитанный `_lumen_loc_parts`, источник
  данных для фикса.
* [BUG-394](BUG-394-FIXED.md) — `Sensor`/подклассы не проверяют собственный
  секьюрити-гейтинг даже отдельно от `isSecureContext` (конструктор вызывается
  напрямую без проверки контекста вообще).
* Прецедент того же класса несовпадения тестового окружения со спекой —
  `audio-output`'s `secure-context.html` (`BUGS.md`, `tests/wpt/VENDOR.md`,
  запись `audio-output`, 2026-07-24).
