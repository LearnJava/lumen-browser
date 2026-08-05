# BUG-650: Permissions Request API (`navigator.permissions.request`/`requestAll`) not implemented at all

**Статус:** OPEN
**Компонент:** js (`crates/js/src/dom.rs:5061-5090` — `navigator.permissions` object literal)
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
vendoring gap (see [BUG-649](bugs/BUG-649-OPEN.md)'s permissions-category note)
— not a new finding.
