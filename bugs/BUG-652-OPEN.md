# BUG-652: `navigator.permissions.revoke` not implemented at all

**Статус:** OPEN
**Компонент:** js (`crates/js/src/dom.rs:5077-5086` — `navigator.permissions` object literal)
**Найден:** P2, WPT-VENDOR-permissions-revoke, 2026-08-05

## Симптом

`permissions-revoke` (скоуп ⬜, кандидат) — вендорена и прогнана целиком
(`run_report.py --all --root permissions-revoke --recursive`, ~14 с, 1 id):
**0/1 harness OK**. The category's single test, `idlharness.any.js`, TIMEOUT
before reaching its own assertions — `/resources/WebIDLParser.js` and
`/resources/idlharness.js` both 404, the same already-documented vendoring
gap shared with `permissions`/`permissions-request` (see BUG-650). No new
finding there.

A direct live probe (`--mcp-live-port`) shows the underlying reason the
category would fail its own assertions even with the harness resources
vendored:

```js
Object.getOwnPropertyNames(navigator.permissions) // => ["query"]
typeof navigator.permissions.revoke               // => "undefined"
```

## Причина

`crates/js/src/dom.rs:5077` defines `navigator.permissions` as a plain
object literal with a single `query` method (W3C Permissions §5). The
(non-standard, Chromium-only) `revoke(permissionDescriptor)` method this
category's `idlharness.any.js` tests — used by WPT's own test suite to reset
a permission's state between tests — is entirely absent, not merely buggy.
Any page script calling `navigator.permissions.revoke(...)` gets a
`TypeError: ... is not a function` instead of a `Promise<PermissionStatus>`.

Distinct from BUG-650 (`request`/`requestAll`, the WICG Permissions Request
API) — `revoke` is a separate, older non-standard extension to the base
`Permissions` interface, not part of that spec.

## Как воспроизвести

```
navigator.permissions.revoke({ name: 'geolocation' })
  // => throws TypeError: navigator.permissions.revoke is not a function
```

## Предлагаемое исправление

Add a `revoke` method to the `navigator.permissions` object literal
mirroring `query`'s validation, resetting `_perm_denied`/granted-state
bookkeeping to whatever Lumen's default is for that name and resolving with
a fresh `PermissionStatus`. Low priority — `revoke` is non-standard and not
required by any in-scope spec; useful only for a future browser-side
permission-UI reset action.
