# BUG-428 — headless CPU render (`--screenshot`, `--ipc-server`) never composites Canvas 2D bitmaps

**Статус:** FIXED 2026-07-29
**Компонент:** shell (`main.rs::render_source_to_png`) + js (`flush_canvas_updates`)
**Найден:** P3, 2026-07-29, while fixing [BUG-348](BUG-348-FIXED.md) — it was the third
piece of evidence in that bug's report and turned out to be an unrelated, pre-existing gap.

## Симптом

Any `<canvas>` drawn by page JS renders as an empty box in every headless CPU render,
regardless of how the context was obtained. `lumen --screenshot .tmp/x.png
graphic_tests/57-canvas-2d.html` produces six blank UA-background rectangles even though
that page's JS runs to completion (`getElementById('cN').getContext('2d')` returns a real
context and `fillRect` executes — verified with an instrumented probe page that prints its
findings into the DOM).

The committed deterministic reference `graphic_tests/snapshots/cpu/57-canvas-2d.png` is
itself blank, so the CPU-snapshot gate (`scripts/scoped-test.sh` →
`cases::snapshot_cpu`) has this behaviour baked in: canvas content is invisible to it.
The live-window pipeline (`graphic_tests/run.py`) is unaffected — it does render canvas
content (TEST-57's ratchet, [BUG-099](BUG-099-OPEN.md)).

## Причина

Canvas 2D pixels reach paint only through the shell's live event loop: `display_list.rs`
emits `DrawImage { src: "canvas:{nid}" }`, and the bitmap behind that key is uploaded by
`ChromeApp`'s per-frame drain

```rust
// crates/shell/src/main.rs:12569
let canvas_updates = self.drain_query_js(|j| j.flush_canvas_updates()).unwrap_or_default();
// … r.register_image(format!("canvas:{nid}"), Arc::new(image))
```

`render_source_to_png` (`crates/shell/src/main.rs:1398`, the shared core of `--screenshot`
and the `--ipc-server` `Screenshot` command) runs `load_bytes` → `parse_and_layout` →
`paint_ordered` → `Renderer::render_to_image_cpu(…, &parsed.images, …)` and never calls
`flush_canvas_updates` at all — nothing ever registers `canvas:{nid}`, so the `DrawImage`
resolves to nothing and paints transparent (`display_list.rs:6982` — "unregistered →
transparent").

## Ожидаемое

Headless CPU render should drain the JS runtime's canvas updates after the JS pass and
feed them into the image set handed to `render_to_image_cpu`, the same way the live loop
feeds `register_image`. Then `--screenshot`/CPU snapshots would show canvas content and
the CPU-snapshot gate would cover Canvas 2D regressions instead of being blind to them.

## Масштаб

- Every headless canvas screenshot (CI, `--ipc-server` tab screenshots, MCP
  `resource://screenshot` if it shares this path) silently drops canvas content.
- ~~`graphic_tests/snapshots/cpu/*.png` cannot regress-test Canvas 2D … regenerating
  snapshots after the fix will legitimately change `57-canvas-2d.png`~~ — **wrong, corrected
  during the fix.** The deterministic CPU snapshots do **not** go through
  `render_source_to_png`: `cases::snapshot_cpu` renders via `InProcessSession`
  (`crates/driver/src/session.rs`), which never executes the page's `<script>` elements and
  passes an unconditionally **empty** image set to `render_to_image_cpu`. The reference stays
  blank after this fix, and the gate stays blind to Canvas 2D — split out as
  [BUG-429](BUG-429-OPEN.md). Snapshot references are therefore unchanged by this fix.

## Фикс (P3, 2026-07-29)

`render_source_to_png` now drains the JS runtime once after `parse_and_layout` (page scripts
have already run there) and appends the drained bitmaps to the image set handed to
`render_to_image_cpu`:

```rust
let mut images = parsed.images;
images.extend(canvas_updates_as_images(
    parsed.js_ctx.as_ref().map(|js| js.flush_canvas_updates()).unwrap_or_default(),
));
```

`canvas_updates_as_images` (new, `main.rs`) converts a `(nid, w, h, rgba)` drain into
`(String, Arc<Image>)` entries keyed `canvas:{nid}` — the exact string `display_list.rs`
emits into `DrawImage.src`. The live event loop's per-frame drain was moved onto the same
helper, so the key format has one definition on the shell side instead of two copies.

**Проверка.** `lumen --screenshot .tmp/57-after.png graphic_tests/57-canvas-2d.html` on the
fixed build renders all six canvases; per-colour pixel counts match the page's declared
geometry exactly — `#f97316`/`#22c55e` 6000 px each (two `fillRect(120×50)`), `#10b981`
23 400 (180×130), `#ef4444` 15 400 (140×110), `#6366f1` 17 344 + `#fbbf24` 7 856 (180×140
rect minus/plus the r=50 arc), `#ec4899` 9 600 (½·160·120 triangle), `#0ea5e9` 28 716 +
`#fef08a` 3 084 (200×160 fill split by the 6 px stroke). Unit tests
`bug428_canvas_updates_keyed_as_display_list_expects` (key format matches the emitter,
RGBA8 payload preserved) and `bug428_empty_canvas_drain_adds_no_images` (no canvas / no JS
context → image set untouched).
