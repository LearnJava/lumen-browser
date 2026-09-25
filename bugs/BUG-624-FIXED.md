# BUG-624: `navigator` has no backing `Navigator` interface at all — no global constructor, `[object Object]`, all members own-instance data properties

**Статус:** FIXED 2026-09-25 (P3)
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

[BUG-366](BUG-366-FIXED.md)/[BUG-367](BUG-367-FIXED.md)/[BUG-369](BUG-369-FIXED.md)
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

## Исправление (P3, 2026-09-25)

`WEB_API_SHIM` (`crates/js/src/shim/web_api_shim_mid_b.js`) теперь заводит
`function Navigator()` (бросает `TypeError: Illegal constructor`) и создаёт
`navigator` как `Object.create(Navigator.prototype)` с прежними членами.

Переписывать ~40 мест, где модули (`permissions.rs`, `webgpu.rs`,
`navigator_bindings.rs`, `surface_api.rs`, хвост шима, …) вешают члены на
`navigator` с собственными атрибутами, не стали — форма WebIDL собирается в
одной точке: `navigator_bindings::finalize_navigator_interface_v8`, вызываемый в
конце `install_dom` прямо перед BUG-378-печатью, переносит каждое собственное
свойство синглтона на `Navigator.prototype`:

* функция → операция на прототипе (writable/enumerable/configurable);
* прочее значение → readonly-атрибут: геттер без сеттера, отдающий
  сохранённое значение (`[SameObject]` бесплатно);
* аксессор → геттер, вызываемый с экземпляром.

Геттеры brand-checked (`Navigator.prototype.userAgent` и
`get.call({})` → `TypeError`), имя `get <attr>`. На прототипе
`Symbol.toStringTag = 'Navigator'`, `Navigator.prototype` неперезаписываем,
сам `Navigator` — неперечисляемое свойство глобала. У синглтона собственных
свойств не остаётся.

Сопутствующие правки:

* `serial.rs`/`webhid.rs`/`webusb.rs`/`webxr.rs` ставили свой член
  `configurable: false` — его нельзя снять с экземпляра; теперь
  `configurable: true`, как у атрибута WebIDL.
* `user_agent_override_script` (BUG-295, живой путь BiDi
  `emulation.setUserAgentOverride` на уже загруженной странице): присваивание
  `navigator.userAgent = …` на геттер без сеттера молча терялось; теперь
  скрипт присваивает, пока свойство ещё собственное (путь `install_dom`), и
  переопределяет геттер прототипа иначе.

Регрессия — `crates/js/src/dom/tests/v8_bug624_navigator_interface.rs`
(6 тестов). Весь `cargo test -p lumen-js --features v8-backend --lib` зелёный.

Живой WPT (`run_smoke.py`, сборка без фикса против сборки с фиксом, 20 id:
`html/webappapis/system-state-and-capabilities/the-navigator-object/*` +
`html/dom/idlharness.https.html`): `idlharness.https.html?exclude=(Document|Window|HTML.+)`
454/1628 → **493/1628** — ровно 39 подтестов `Navigator interface: …`
FAIL→PASS (существование интерфейсного объекта и прототипа, `length`/`name`,
`@@unscopables`, «primary interface of window.navigator», stringification,
атрибуты `NavigatorID`/`NavigatorLanguage`/`NavigatorOnLine`/`NavigatorCookies`/
`NavigatorPlugins`/`NavigatorConcurrentHardware`, `userActivation`). Остальные
19 id — без изменений, ни одного PASS→FAIL.
