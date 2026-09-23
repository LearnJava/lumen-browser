# BUG-1101 — crates.io рендерится пустой белой страницей из-за `[unhandled-rejection] TypeError: Cannot read properties of undefined (reading 'get')`

**Статус:** FIXED 2026-09-23 (P6)
**Тип:** дефект движка (JS shim, DOM prototype chain).
**Заведён:** 2026-09-23 (P6, RP-10 — повторный аудит реальных сайтов против Edge).
**Область:** `crates/js/src/shim/web_api_shim_mid.js` — `_lumen_wrapper_proto_for`/`_LUMEN_WRAPPER_MEMBERS`.

## Симптом

Живой прогон `/lumen-perf-audit --mode compat` (RP-10, `d530491e1`): `crates.io`
проходит TLS/антибот-проверку (`← 200 https://crates.io/`), но итоговый кадр —
**пустая белая страница**, `--dump-layout` даёт `[unhandled-rejection]
TypeError: Cannot read properties of undefined (reading 'get')` сразу после
`GET .../api/v1/site_metadata`/`.../api/v1/summary` (оба ещё в полёте).

## Причина (подтверждена, не гипотеза)

Диагностика: `crates/js/src/v8_runtime/promise_reject.rs` получил вывод
`reason.stack` рядом с уже существующим `[unhandled-rejection]` логом
(оставлено как постоянное улучшение — полезно для любого будущего
`unhandledrejection` на живом сайте). Стек указал точное место в минифицированном
бандле crates.io (`_app/immutable/chunks/DcCnarDz.js`, Svelte 5 runtime):

```js
function Ln(){ ... var t=Node.prototype; ...
  Fn=o(t,`firstChild`).get, In=o(t,`nextSibling`).get; ... }
```

(`o` = `Object.getOwnPropertyDescriptor`). Svelte 5's hydration/DOM-walk
runtime grabs the *native* `firstChild`/`nextSibling` accessor directly off
`Node.prototype` once (`R(e)=Fn.call(e)`, `z(e)=In.call(e)`) — a common
anti-monkey-patch pattern, not specific to crates.io (any Svelte-5-hydrated
page walking the DOM via `R`/`z` hits the same wall).

Lumen's DOM §4.4 accessors for a live node (`firstChild`, `nextSibling`,
`childNodes`, `parentNode`, …) are real and work fine via `el.firstChild` —
but they live on a **hidden per-interface prototype** built by
`_lumen_wrapper_proto_for(iface, kind)` (`Object.create(iface)` +
`Object.defineProperties(proto, _LUMEN_WRAPPER_DESCRIPTORS)`), sitting
*between* a wrapper instance and its interface prototype
(`HTMLDivElement.prototype` etc.) — never on `Node.prototype` itself, even
though DOM §4.4 places them there and other Node-level members
(`baseURI`, `hasChildNodes`, `contains`) already do live on
`Node.prototype` directly (deliberate perf tradeoff documented at the call
site: "one object per interface instead of one property set per node").
`Object.getOwnPropertyDescriptor(Node.prototype, 'firstChild')` therefore
legitimately answered `undefined`, and `.get` on that crashed exactly as
V8's own message says.

Verified directly: a local repro page reading
`Object.getOwnPropertyDescriptor(Node.prototype, 'firstChild')`/`'nextSibling'`
reported `MISSING` before the fix, `"function"` after.

## Фикс

`crates/js/src/shim/web_api_shim_mid.js`: reuse the exact same descriptor
objects already computed for the per-interface proto
(`_LUMEN_WRAPPER_DESCRIPTORS.firstChild`/`.nextSibling`) and additionally
install them on `Node.prototype` via `Object.defineProperties`. Purely
additive — the per-interface proto still sits nearer in the chain for
ordinary `el.firstChild` reads, so behaviour is unchanged; this only adds
what real-DOM introspection (`getOwnPropertyDescriptor` on `Node.prototype`)
should already find. `dump_golden.py`'s mismatch set is identical
before/after the patch (see gate note below) — display-list neutral.

## Проверка

- Локальный репро (`fetch(...).then(r => { var x; x.get('foo'); })`) — стек
  теперь содержит реальные фреймы (было пусто до диагностики).
- `crates.io`: `[unhandled-rejection] TypeError: Cannot read properties of
  undefined (reading 'get')` **исчез**. Страница продвигается дальше и
  упирается в уже заведённый [BUG-1092](BUG-1092-OPEN.md) (`SVGAElement`
  и ещё 12 отсутствующих SVG WebIDL-глобалов) — отдельный, уже
  локализованный дефект, полный рендер crates.io требует и его тоже.
- `cargo clippy -p lumen-js --all-targets --features v8-backend -- -D
  warnings` чист.
- `scripts/scoped-test.sh` — три `dom::tests::v8_webworker::*` не проходят
  под параллельным раннером, но зелены поодиночке (`--test-threads=1`) —
  подтверждённый флейк раннера, не регрессия правки.
- `LUMEN_PROFILE=dev-release python graphic_tests/dump_golden.py` — **гейт
  сейчас красный на чистом `main` независимо от этой правки** (см.
  [BUG-1106](BUG-1106-OPEN.md)); множество несовпадений с патчем и без
  патча совпадает дословно (8 из 12, те же страницы, тот же паттерн) —
  правка display-list-нейтральна относительно текущей (сломанной) базовой
  линии.

## Побочная находка

Гейт `dump_golden.py` красный на свежепересобранном `main` независимо от
этой правки — заведено отдельно как [BUG-1106](BUG-1106-OPEN.md), не
расширяет эту карточку.
