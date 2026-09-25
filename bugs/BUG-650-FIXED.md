# BUG-650: Permissions Request API (`navigator.permissions.request`/`requestAll`) not implemented at all

**Статус:** FIXED 2026-09-25 (P3)
**Компонент:** js (`crates/js/src/permissions.rs` — `PERMISSIONS_SHIM`, `Permissions.prototype`)
**Найден:** P2, WPT-VENDOR-permissions-request, 2026-08-05

## Симптом

`permissions-request` (скоуп ⬜, кандидат) — вендорена и прогнана целиком
(`run_report.py --all --root permissions-request --recursive`, ~14 с, 1 id):
**0/1 harness OK**. The category's single test, `idlharness.any.js`, TIMEOUT
before reaching its own assertions — `/resources/WebIDLParser.js` and
`/resources/idlharness.js` both 404 (a known, already-documented vendoring gap
shared with every other `idlharness.any.js`-based category, e.g.
`permissions`'s own idlharness test hit the same gap). No new finding there.

A direct live probe (`--mcp-live-port`, `navigator.permissions.*`) shows the
underlying reason the category would fail its own assertions even with the
harness resources vendored:

```
typeof navigator.permissions            => "object"
typeof navigator.permissions.query      => "function"
typeof navigator.permissions.request    => "undefined"
typeof navigator.permissions.requestAll => "undefined"
Object.getOwnPropertyNames(navigator.permissions) => ["query"]
navigator.permissions.request({name:'geolocation'})
  => throws TypeError: navigator.permissions.request is not a function
```

## Причина

`crates/js/src/dom.rs:5077` defines `navigator.permissions` as a plain object
literal with a single `query` method (W3C Permissions §5). The WICG
[Permissions Request API](https://wicg.github.io/permissions-request/) spec
this category tests extends `Permissions` with `request(permissionDescriptors)`
and `requestAll(permissionDescriptors)` — both entirely absent, not merely
buggy. Any page script calling `navigator.permissions.request(...)` gets a
`TypeError: ... is not a function` instead of a `Promise<PermissionStatus>` (or
`Promise<PermissionStatus[]>` for `requestAll`).

## Как воспроизвести

```
tests/wpt/run_report.py --binary <lumen.exe> --all --root permissions-request --recursive
```
or a live probe (`--mcp-live-port`, page with a `<script>` tag so the JS
runtime installs):
```js
typeof navigator.permissions.request     // "undefined", spec: "function"
typeof navigator.permissions.requestAll  // "undefined", spec: "function"
```

## Реконфирмации / связанные категории (не заведены отдельно)

The category's only executed harness result (TIMEOUT on `idlharness.any.html`)
is the already-documented `/resources/idlharness.js`+`/resources/WebIDLParser.js`
vendoring gap (see [BUG-649](BUG-649-FIXED.md)'s permissions-category note)
— not a new finding.

## Исправление (P3, 2026-09-25)

Заявка устарела в двух местах, прежде чем её чинить. `navigator.permissions` —
давно не литерал в `dom.rs`, а модуль `crates/js/src/permissions.rs`
(BUG-386): реестр имён, `PermissionStatus` как `EventTarget`. А `requestAll()`
из текущего черновика WICG выброшен: вендоренный `interfaces/permissions-request.idl`
объявляет ровно `Promise<PermissionStatus> request(object permissionDesc)`.
Поэтому ставится только `request()`; `requestAll` не добавляется (его нет и в
Chromium).

`Permissions.prototype.request(permissionDesc)`:

* дескриптор проходит ту же WebIDL-конверсию, что у `query()` (общий
  `readDescriptor`, сообщение называет свою операцию), и любая ошибка —
  отклонённый промис, не синхронный `throw`, включая вызов без аргументов;
* «request permission to use» спрашивать есть кого только в состоянии
  `prompt`; `granted`/`denied` — окончательные ответы, статус возвращается
  как есть;
* `prompt` сегодня бывает только у `notifications`, и вопрос уходит в
  `Notification.requestPermission()` — владельца ответа, — поэтому
  `navigator.permissions` и `Notification.permission` не расходятся, а
  ранее выданные статусы получают `change` по штатному пути BUG-386.
  Упавший запрос — не грант: статус отражает то, что говорит API после него.

Попутно: WebIDL §3.7.6 требует у регулярной операции `enumerable: true`, а
`query` (и первая версия `request`) ставились через `def()` с `false` —
idlharness падал на «property should be enumerable». Обе операции теперь идут
через `defOp()`.

Тесты (`permissions.rs`): `request_is_an_enumerable_prototype_operation_of_length_one`,
`request_resolves_granted_and_denied_names_unchanged`,
`request_rejects_bad_descriptors_without_throwing`,
`request_asks_the_notification_api_while_prompt`,
`request_survives_a_failing_prompt`.

WPT `permissions-request`: 0/1 → **2/2 harness OK, 11/14 сабтестов**; окно
зелёное целиком, `idl_test setup` больше не падает. Три оставшихся — в
`idlharness.any.worker.html`: у `WorkerNavigator` нет `permissions` вовсе,
это отдельный пробел, заведён [BUG-1174](BUG-1174-OPEN.md). Эталон
`tests/wpt/metadata/permissions-request/idlharness.any.js.ini` перегенерирован
(`--update-expected`, затем `--check`: 0 регрессий).
