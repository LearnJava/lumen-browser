# BUG-348 — `canvas.getContext('2d')` returns `null` unconditionally under the V8 default build: WebGL shim clobbers the 2D context accessor

**Статус:** OPEN
**Компонент:** js (`crates/js/src/webgl_canvas.rs`, `crates/js/src/v8_runtime.rs`)
**Найден:** P2, WPT-VENDOR-density-size-correction (2026-07-26), while triaging a `TypeError: Cannot set properties of null (setting 'fillColor')` and a TIMEOUT in the vendored `density-size-correction` category — both tests do nothing more exotic than `document.createElement('canvas').getContext('2d')`.

## Симптом

`HTMLCanvasElement.getContext('2d')` returns `null` for **every** canvas, appended or
detached — not a WPT/BiDi-specific quirk. Confirmed three ways:

1. Two `density-size-correction` WPT tests fail/hang on the very first
   `canvas.getContext('2d')` call (`resources/exify.js`'s `createImageWithMetadata`,
   `density-corrected-image-in-canvas.html`'s `createCanvasWithImage`).
2. An isolated repro over plain WebDriver BiDi (`--bidi-port`, no wptrunner/wptserve
   involved at all) against a bare `file://` page: both a detached and a
   `document.body`-appended canvas return `ctx === null`, while
   `_lumen_get_tag_name`/`_lumen_canvas_is_transferred`/`_lumen_canvas_dims` all report
   correct values (`tag: "canvas"`, `transferred: false`, `dims: [10, 20]`) for the same
   node id in the same synchronous eval — ruling out both of `getContext`'s own
   null-return guards (`crates/js/src/dom.rs:5963-5964`).
3. `lumen --screenshot .tmp/canvas2d-check.png graphic_tests/57-canvas-2d.html`
   (CPU renderer, **not** BiDi) renders all six canvases as blank UA-background
   rectangles — no `fillRect`/`arc`/path/`strokeRect` content at all. So this is not
   headless/BiDi-specific; it's the current state of `main`'s default (V8) build.

## Причина

`crates/js/src/v8_runtime.rs:3858-3865` installs canvas-related JS shims in this order
(comment: "Mirrors `lib.rs::install_dom`'s ordering (webgl before canvas2d)"):

```rust
install_webgl_canvas_v8(self, &fingerprint)?;   // webgl_canvas.rs
install_v8!(canvas2d::install_canvas2d_bindings_v8);
```

`webgl_canvas.rs`'s `WEBGL_SHIM` (used by both the QuickJS and V8 ports) monkey-patches
`document.createElement`:

```js
// webgl_canvas.rs:484-493
var _origCreate = document.createElement.bind(document);
document.createElement = function(tag) {
  var el = _origCreate(tag);
  if (typeof tag === 'string' && tag.toLowerCase() === 'canvas') {
    _addCanvasStubs(el);   // <-- overwrites el.getContext unconditionally
  }
  return el;
};
```

and `_addCanvasStubs` (`webgl_canvas.rs:462-478`) sets `el.getContext` to a function that
only handles `'webgl'/'webgl2'/'experimental-webgl'` and **returns `null` for every other
`contextType`, including `'2d'`**:

```js
el.getContext = function(contextType) {
  var t = ('' + (contextType || '')).toLowerCase();
  if (t === 'webgl' || t === 'webgl2' || t === 'experimental-webgl') { ... return _ctx; }
  return null;
};
```

The real, functional `'2d'`/`'bitmaprenderer'`/`'webgpu'` implementation lives in
`crates/js/src/dom.rs:5959-6013`, installed as part of the base element-wrapper factory
(`WEB_API_SHIM`, ordinary `document.createElement`) — i.e. *before* `webgl_canvas`'s
wrapper runs. `canvas2d.rs`'s own V8 install (`install_canvas2d_bindings_v8`) registers
only the native `_lumen_canvas2d_*` bindings the `dom.rs` shim's `getContext('2d')`
branch calls into — it does **not** touch `document.createElement`/`getContext` itself
(confirmed: no match for either in `canvas2d.rs`). So the WebGL wrapper's own-property
`el.getContext` assignment is the *last* write and permanently shadows the working `2d`
accessor for every canvas created after JS-runtime bootstrap — there is nothing
downstream that restores it.

This is a straightforward install-order bug from `P3-v8-s8` (`26309f646`, 2026-07-14,
same day as the V8 default cutover, ADR-018) — the QuickJS rollback path shares the same
`WEBGL_SHIM` source and is presumably equally affected, but V8 is what every graphic
test / WPT run since 2026-07-14 has actually exercised.

## Предлагаемый фикс (not applied — filing only, P2 does not own `crates/js`)

`_addCanvasStubs`'s `getContext` override should fall through to whatever
`document.createElement`'s *original* wrapper already put on `el` for any
`contextType` it doesn't itself handle, instead of unconditionally returning `null`:

```js
el.getContext = function(contextType) {
  var t = ('' + (contextType || '')).toLowerCase();
  if (t === 'webgl' || t === 'webgl2' || t === 'experimental-webgl') { ... return _ctx; }
  return _origGetContext ? _origGetContext.call(el, contextType) : null;
};
```

capturing `el.getContext` (the dom.rs-installed one) as `_origGetContext` before
overwriting it. Needs a matching fix in both the QuickJS (`install_webgl_canvas`) and V8
(`install_webgl_canvas_v8`) shim strings since they share `WEBGL_SHIM` verbatim — and a
regression test/graphic-test rerun of `57-canvas-2d.html` to confirm the fillRect/arc/
path/strokeRect boxes actually render again.

## Масштаб

Breaks `<canvas>` 2D entirely for the current default engine build — not just this WPT
category. Every graphic test, sample page, or real site that uses `CanvasRenderingContext2D`
is affected. `graphic_tests/57-canvas-2d.html`'s CPU-screenshot snapshot renders blank
placeholders instead of its fillRect/arc/path/strokeRect boxes (visually confirmed
2026-07-26, `.tmp/canvas2d-check.png`, not committed).

## Уточнение охвата (P2, 2026-07-28, WPT-VENDOR-html-canvas) — сломан один путь создания из четырёх

Заголовок «unconditionally / for **every** canvas» **неверен**, и это меняет и приоритет,
и способ проверки фикса. Патчится не `HTMLCanvasElement`, а один метод `document`,
поэтому шим `_addCanvasStubs` достаётся ровно тем элементам, что прошли через
`document.createElement`. Проба (`--dump-layout`, один и тот же `fillRect` во все четыре
канваса):

```
parsed.2d=true     # <canvas> из разметки          — рисует, getImageData даёт 18,52,86,255
made.2d=false      # document.createElement        — null (этот баг)
madeNS.2d=true     # document.createElementNS(xhtml)— рисует
cloned.2d=true     # made.cloneNode(false)         — рисует (клон «сломанного» элемента цел)
```

Зеркальное следствие: `webgl` доступен **только** на `createElement`-канвасе
(`parsed.webgl=null`), то есть два шима не сосуществуют, а делят элементы по пути
создания. Оба факта воспроизводятся одной страницей без автоматизации.

Пункт 3 исходного описания (blank-снимок `57-canvas-2d.html`) **не является
свидетельством этого бага**: страница берёт канвасы через `getElementById`, то есть
рабочим путём, а headless-путь `--screenshot`/`render_source_to_png` выполняется с
выключенным интерактивным JS (`crates/shell/src/main.rs:1437` — `false, // headless: no
interactive JS`) и без регистрации пиксельных источников, поэтому там любой `<canvas>`
рисуется серым плейсхолдером `DrawImage` по построению (это же зафиксировано в
шапке `crates/driver/tests/cases/snapshot_cpu.rs`). Blank-снимок и коммиченный эталон
`graphic_tests/snapshots/cpu/57-canvas-2d.png` — ожидаемое поведение headless-режима, а
не признак поломки Canvas 2D. Живой прогон TEST-57 идёт по разметочному пути и потому
этим багом не затронут ([BUG-099](BUG-099-OPEN.md), долг 2.96 % — параметры шрифта и AA).

Проверять фикс, соответственно, надо не снимком, а страницей с обоими путями создания
(и `webgl` на разметочном канвасе), либо прогоном `html/canvas`.

Практическая цена по WPT (срез `html/canvas`, 2026-07-28): канонические
сгенерированные тесты берут `<canvas id="c">` из разметки (`canvas-tests.js::_addTest`)
и потому исполняются, а на `document.createElement` завязаны более новые файлы —
88 из 1730 в `element/` — и вся серия `the-canvas-state` (23 id, 62 из 68 сабтестов
падают `Cannot read/set properties of null`). Точные числа — в строке
`WPT-VENDOR-html-canvas` ROADMAP.md.

Та же строка `_addCanvasStubs` порождает [BUG-419](BUG-419-OPEN.md): анти-fingerprint
заглушка `toDataURL`/`toBlob` живёт в этом же патче и потому закрывает ровно один путь
создания элемента из четырёх.
