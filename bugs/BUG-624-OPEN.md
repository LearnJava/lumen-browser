# BUG-624: `navigator` has no backing `Navigator` interface at all — no global constructor, `[object Object]`, all members own-instance data properties

**Статус:** OPEN
**Компонент:** js (`crates/js/src/dom.rs:4999` — `var navigator = { ... }` object literal)
**Найден:** P2, WPT-VENDOR-installedapp, 2026-08-05, проба `--mcp-live-port`

## Симптом

Confirmed live (`--mcp-live-port`, `eval`):

```
typeof window.Navigator                          = "undefined"
Object.prototype.toString.call(navigator)         = "[object Object]"
Object.getOwnPropertyDescriptor(navigator,
  'userAgent')                                    = {value:"Lumen/0.5.0", writable:true,
                                                       enumerable:true, configurable:true}
Object.getOwnPropertyNames(
  Object.getPrototypeOf(navigator))                = only generic Object.prototype members
                                                       (constructor, hasOwnProperty, toString, …)
```

`navigator` (`dom.rs:4999`) is a plain `{...}` object literal, not
`Object.create(Navigator.prototype)`. Every one of its ~48 members
(`userAgent`, `language`, `onLine`, `clipboard`, `permissions`, `geolocation`,
`mediaDevices`, `serviceWorker`, `credentials`, `share`, `getBattery`, …) is
a writable/enumerable/configurable own data property of the singleton
instance, not a getter on an interface prototype.

## Отличие от класса BUG-366

[BUG-366](BUG-366-FIXED.md)/[BUG-367](BUG-367-FIXED.md)/[BUG-369](BUG-369-OPEN.md)
document the same instance-vs-prototype defect for *sub-objects hanging off*
`navigator` (`navigator.credentials`, `Headers`, `Element`) — those interfaces
at least exist as global constructors (`CredentialsContainer`, `Headers`),
just with methods misplaced. Here there is no `Navigator` global at all:
`window.Navigator` is `undefined`, so `navigator instanceof Navigator` cannot
even be expressed, and `navigator`'s own shape can never be corrected by
fixing a prototype in isolation — the interface object itself needs to be
created first.

## Масштаб

Affects every WPT category that inspects `navigator`'s WebIDL shape directly
(idlharness tests against `Navigator`, `instanceof` checks, `for...in`/
`Object.keys` enumeration of the global navigator singleton) — not specific
to `installedapp`, but first surfaced there because that category's own API
(`getInstalledRelatedApps`, 🚫-scope, correctly `undefined`) forced a probe of
the shared `navigator` container per the "probe container object" WPT-VENDOR
convention. Not investigated: how many currently-passing subtests elsewhere
in the vendored corpus rely on today's plain-object shape and would need
re-verification if this is fixed (`instanceof`-based feature detection would
flip from false-negative to correct, but any test asserting exact enumerable
key sets could shift).
