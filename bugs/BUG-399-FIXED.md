# BUG-399 — `window.isSecureContext` захардкожен `true` для любой страницы, не вычисляется из протокола/хоста

**Статус:** FIXED 2026-08-11 (P3)
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

## Исправление (P3, 2026-08-11)

Литерал заменён вычислением из URL, с которым документ был установлен.
`_lumen_url_is_potentially_trustworthy(parts)` (`crates/js/src/dom.rs`, рядом с
`_lumen_loc_parts`) реализует §3.1/§3.2: `https`/`wss`/`file` → доверенный;
`about:blank`/`about:srcdoc` → доверенный (наследуют контекст создателя),
любой другой `about:` — нет; `data:` → доверенный по §3.2 несмотря на opaque
origin; `blob:` — рекурсивно по origin, который он несёт; иначе — проверка
хоста на loopback (`localhost` целиком-меткой, включая поддомены и форму с
завершающей точкой, 127.0.0.0/8, IPv6 `::1` со всеми формами сжатия).

Две детали, которые пришлось сделать не так, как предлагала заявка:

* **Схема берётся из `href`, а не из `parts.protocol`.** `_lumen_parse_url` —
  не полный URL-парсер: он режет по первому `://`, поэтому `blob:https://h/id`
  даёт ему протокол `blob:https:`, а `data:`-URL, в теле которого встретилось
  `://`, — вообще мусор. Схема по URL Standard — всё до первого двоеточия, так
  она здесь и читается. Проверка по `parts.protocol` (буквально то, что
  предлагал раздел «Что нужно сделать») отдала бы `false` на `blob:https://…`.
* **Значение снимается один раз при установке рантайма, а не читается на
  каждом обращении.** Флаг принадлежит environment settings object и
  фиксируется при создании документа (HTML LS §8.1.5.1), а `install_dom`
  выполняется ровно на документ. Живое чтение было бы вдобавок активно
  неверным: same-document `history.pushState(s, '', '/x')` кладёт в
  `_lumen_loc_parts` сырую относительную строку (см. `_lumen_location_update`),
  и флаг на https-странице перевернулся бы в `false`. Закреплено тестом
  `is_secure_context_survives_same_document_navigation`.

Свойство отдаётся getter-only аксессором из замыкания: WebIDL объявляет
`readonly attribute boolean`, поэтому присваивание со стороны страницы не
должно отвечать за движок (класс [BUG-366](BUG-366-FIXED.md)); замыкание
выбрано вместо `_lumen_…`-глобала, потому что `seal_internal_globals_v8`
оставляет движковое *состояние* записываемым.

Осознанная асимметрия: хост сопоставляется как написан — сокращённый
IPv4-литерал (`127.1`) `_lumen_parse_url` не нормализует, и такая форма
получает «не доверенный». Ложноотрицательный ответ отказывает в гейтящемся
API, ложноположительный — выдал бы его на небезопасном origin; ошибаться
безопасно только в первую сторону. По той же причине рантайм без URL страницы
вовсе (форма большинства юнит-тестовых рантаймов) — не доверенный.

**Проверка.** 5 юнит-тестов вместо прежнего `is_secure_context_is_true`
(14 доверенных URL, 10 недоверенных, отсутствие URL, неперезаписываемость,
устойчивость к `pushState`) — `cargo test -p lumen-js --features v8-backend
is_secure_context`, 5/5. Юнит-тесты бьют по `install_dom` напрямую, поэтому
дополнительно прогнана живая проба на собранном `lumen.exe` (`--mcp-port` +
локальный HTTP-сервер, слушающий и loopback, и LAN-адрес): `http://127.0.0.1`
→ `true`, `http://localhost` → `true`, `http://<LAN-IPv4>` → `false`,
`file://…` → `true` — 4/4. Формы `file:///D:/…` и `about:blank` в headless-MCP
недостижимы по уже заведённому [BUG-760](BUG-760-OPEN.md) (`navigate` там не
знает ни трёхслэшевой формы, ни схемы `about:`), поэтому проверены только
юнит-тестами.

**Что этот фикс НЕ делает** (осталось за границей заявки, заведено отдельно):

* Флаг теперь верен, но его никто не читает: гейта `[SecureContext]` в
  движке по-прежнему нет ни у одного API (`grep isSecureContext crates/` вне
  `dom.rs` — пусто), так что Web Crypto/Clipboard/сенсоры/Service Workers
  доступны с `http://`-страницы как и раньше — [BUG-765](BUG-765-OPEN.md).
  Именно этот разрыв делал ложноположительным `assert_false` в
  `Gyroscope_insecure_context.html`, с которого началась заявка: чтобы тест
  позеленел на не-loopback origin, нужен не только флаг, но и гейт.
* В `WorkerGlobalScope` свойства нет вовсе (`worker_global_shim` его не
  заводит) — [BUG-766](BUG-766-OPEN.md), тот же класс, что
  [BUG-401](BUG-401-FIXED.md).

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
