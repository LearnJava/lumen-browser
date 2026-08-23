# BUG-630: `<img>` never fires `load`/`error`, and `HTMLImageElement` has no `complete`/`naturalWidth`/`naturalHeight`/`onload`/`onerror` at all — for every image format, not just JPEG XL

**Статус:** OPEN
**Компонент:** js (`crates/js/src/dom.rs:10864-10875` — `HTMLImageElement.prototype` gets only `_lumen_install_reflection` attribute reflection, no decoded-state accessors, no event-handler IDL attributes), shell (`crates/shell/src/main.rs:4892-4904` `decode_image`, `crates/shell/src/main.rs:10630-10635` lazy-load path, `crates/shell/src/main.rs:5507-5512` background-image path — all three log-and-drop a decode `Err`/`Ok` with no signal back to JS)
**Найден:** P2, WPT-VENDOR-jpegxl, 2026-08-05

## Симптом

Confirmed via two live `--mcp-live-port` probes (`window.__probe_done` polling pattern):

1. A `<img>` pointed at a **valid, decodable PNG** never fires `load` or `error`
   within 8 s, even though the image visibly decodes and lays out correctly
   (`width` reflects the real intrinsic size, 3px for a 3×3 fixture).
2. `'complete' in img`, `'naturalWidth' in img`, `'naturalHeight' in img`,
   `'onload' in img`, `'onerror' in img` are all `false` — checked both on the
   instance and on `Object.getPrototypeOf(img)`. None of these five members
   exist anywhere in the prototype chain. `Object.getOwnPropertyNames(img)`
   lists dozens of generic Element/Node/form/canvas members but not one of
   them.

So this is not a JPEG-XL-specific gap (JXL just happens to be the one format
whose decoder is a Phase-0 stub that always fails, which is what surfaced this
while investigating `WPT-VENDOR-jpegxl`'s TIMEOUT wall) — it is a **general,
format-independent absence of the entire decoded-image-state IDL surface**.
Any script relying on `img.complete`, `img.naturalWidth/Height`,
`img.onload =`, or `img.addEventListener('load'/'error', ...)` gets nothing,
for any image, successful decode or failed decode alike.

## Причина

`HTMLImageElement.prototype` (`dom.rs:4150-4152`) is built from a bare
`Object.create(HTMLElement.prototype)` and then only gets attribute
reflection (`dom.rs:10864-10875`: `src`/`alt`/`srcset`/`sizes`/`useMap`/
`isMap`/`crossOrigin`/`decoding`/`loading`/`referrerPolicy`) — no
`complete`/`naturalWidth`/`naturalHeight` getters were ever added, matching
the pattern for `body`/`head` in [BUG-485](BUG-485-FIXED.md)/[BUG-565](BUG-565-FIXED.md)
(a member simply never wired up, not a deliberate stub).

On the engine side, the single fetch+decode entry point
(`crates/shell/src/main.rs::decode_image`, used by both the eager and
streaming/lazy pipelines per its own doc comment) converts both branches to a
dead end for script:
- `Err(e)` (`main.rs:4900-4903`): `eprintln!("Не декодируется {raw_src}: {e}")`
  then `None` — the same generic pattern repeats at the lazy-load call site
  (`main.rs:10630-10635`, `"Lazy: не декодируется {url}"`) and the
  background-image call site (`main.rs:5507-5512`). `None` propagates through
  `image_cache::IMAGE_CACHE.get_or_decode_current` to `ImgOutcome::Skip`
  (`main.rs:4727,4747` in `fetch_and_decode_images`), a silent no-op — no DOM
  node touched, nothing queued for JS to observe.
- `Ok(image)` (`main.rs:4893-4899`): also just logs and returns the decoded
  `Image` for internal layout/paint use — no corresponding dispatch of a
  `load` event or update to any per-element decoded-state flag exists
  anywhere in `crates/shell/src/main.rs` or `crates/js/src/dom.rs` (grepped
  for `dispatch_event`/`fire_event` near image code — zero hits tied to
  image load completion).

Decode success or failure is therefore **entirely invisible to page script**
on both branches, not just the failure branch.

## Масштаб

This is a foundational gap that any WPT category exercising `<img>` loading
hits — `promise_test`s that `await` an image's `load`/`error` event (the
single most common idiom for image-related tests, see e.g. `jpegxl`'s
`html-img.html`/`html-input-image.html`/`alpha-vardct.html`/
`lossless-jpeg-transcode.html`/`modular-lossy.html`/`svg-image-element.html`/
`wide-gamut-*.html`, all TIMEOUT) hang until wptrunner's outer timeout instead
of resolving promptly either way. `CAPABILITIES.md`'s image-loading line
should be checked for the same ✅→🟡 drift class as [BUG-368](BUG-368-OPEN.md).

## Что нужно

1. Add `get complete()`, `get naturalWidth()`, `get naturalHeight()` to
   `HTMLImageElement.prototype` (`dom.rs:~10864`), backed by native bindings
   that read the per-element decoded-image state (or its absence) — same
   pattern as `_lumen_get_body`/`_lumen_get_html_element`.
2. Add `onload`/`onerror` IDL event-handler attributes (same
   `content_attribute`-less pattern as other `on*` handlers already present
   elsewhere in the file, e.g. `onloadstart`/`onload`/`onloadend` at
   `dom.rs:8106-8111` for a different interface).
3. Wire an actual `load`/`error` DOM event dispatch into
   `crates/shell/src/main.rs`'s three decode call sites (`decode_image`'s
   `Ok`/`Err` branches, the lazy-load path, the background-image path) so a
   real event fires on the owning `<img>`/CSS-background element once decode
   settles, matching HTML LS §4.8.4.3 timing (`error` for the `Err` branch,
   `load` for `Ok`).

## Related

- [BUG-569](BUG-569-OPEN.md): `HTMLImageElement.prototype.decode()` missing —
  narrower (one method), but same root cause class (no async decode
  task/state to hook into).
- [BUG-360](BUG-360-FIXED.md): inline `onclick=`-style attribute handlers never
  compile — different mechanism (affects attribute-style handlers even when
  the underlying event *would* fire); does not explain this bug, since here
  `addEventListener('load'/'error', ...)` never fires either, because no
  dispatch exists at all, attribute-style or not.

## WPT-RUN-6 срез 5 (`html/semantics/embedded-content/the-img-element`, 2026-08-21)

Still fully reproducible 16 days later: `grep -n "naturalWidth\|naturalHeight" crates/js/src/dom.rs`
— zero matches; no `onload` wiring tied to `<img>` either. Fresh category run
(161 files, `run_report.py --all --root html/semantics/embedded-content/the-img-element
--recursive --processes 6`, 3 min): **45/131 TIMEOUT (34.4%)** — 18 in
`image-loading-lazy-*` (a genuinely-implemented feature, see the
`// ── Lazy image loading (HTML LS §2.6.6.9) ──` block in `dom.rs:11486+`,
but its completion is still signalled the same missing way), the remaining
27 spread across `img.complete.html`, `data-url.html`,
`natural-size-orientation.html`, `already-loaded-image-sync-width.html`,
`invalid-src.html`, `nonexistent-image.html`, etc. Sampled both groups —
every file awaits `img.onload` or reads `img.complete`/`naturalWidth`, same
single root cause across the whole category, not two separate gaps. No new
`BUG-NNN` filed.

## Перезамер 2026-08-23 (WPT-RUN-6, срез 27): молчат обе формы, включая атрибут

`tests/wpt/verify_callback_import_preload_gaps.py --variant img-onload-attr`
на `main` = `34cbefd25`. На одной странице четыре формы ожидания сразу:
`onload=`-атрибут на `<img>`, написанном парсером; `addEventListener('load')`
на нём же; `onload`-атрибут, выставленный через `setAttribute` на созданном
скриптом `<img>`; и слушатель на нём. Сервер пробы отдаёт обе картинки:

```
ioa-parser-onload=function complete=undefined
ioa-script-appended
ioa-checked
[server saw: GET /vcip-pixel.png, GET /vcip-pixel.png?script-made]
```

Ни один из четырёх маркеров загрузки не напечатан. То есть форма ожидания ни
на что не влияет — событие не диспатчится вовсе, и картинка, созданная
скриптом, ведёт себя ровно как написанная парсером (это уточняет прежнюю
формулировку, где различались вставленные парсером и скриптом узлы).
Побочно: `img.complete` — `undefined`, то есть синхронный обходной путь
(`if (img.complete) …`), которым пользуется часть тестов, тоже не работает.

Цена по остатку WPT-RUN-5, сверх прежней: `import-maps/no-referencing-script-integrity.html`
и `…-valid.html` — оба запускают `import()` из `onload`-атрибута
парсерного `<img src="/images/green.png">`, то есть до самой карты импортов
дело не доходит.

## Перезамер (WPT-RUN-6, срез 28, 2026-08-23)

`verify_window_history_jsurl_gaps.py --variant canvas-misc`: `<img>`,
созданный скриптом и указывающий на SVG-файл, который сервер пробы отдаёт,
не диспатчит ни `load`, ни `error` за 8 секунд, `img.complete` —
`undefined`. Это, а не сам canvas, и есть причина зависания
canvas-семейства: `ctx` создаётся, `ctx.filter` читается (`"none"`) и
пишется (`"blur(2px)"`), `OffscreenCanvas`,
`canvas.transferControlToOffscreen` и `createImageBitmap` — все три
существуют, а рисовать нечего, потому что картинка о своей загрузке молчит.
