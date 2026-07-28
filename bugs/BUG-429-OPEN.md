# BUG-429 — the CPU-snapshot gate renders pages without running their scripts and without any images

**Статус:** OPEN
**Компонент:** driver (`crates/driver/src/session.rs::run_pipeline` / `screenshot_cpu_rgba`)
**Найден:** P3, 2026-07-29, while fixing [BUG-428](BUG-428-FIXED.md) — BUG-428's «Масштаб»
section assumed the deterministic CPU snapshots share the shell's headless render path.
They do not; this is what actually stands between that gate and Canvas 2D coverage.

## Симптом

`graphic_tests/snapshots/cpu/57-canvas-2d.png` is blank where the canvas bitmaps should be,
and BUG-428's fix (draining `flush_canvas_updates` into the shell's `--screenshot` image set)
does **not** change it: the snapshot gate never reaches that code.

## Причина

`cases::snapshot_cpu` renders through `InProcessSession` (`lumen-driver`), not through the
shell:

* `run_pipeline` (`crates/driver/src/session.rs:263`) parses HTML, collects `<style>` blocks,
  lays out and builds the display list. It installs a V8 runtime with DOM bindings
  (`new_v8_runtime`, line 992) so that `click`/`type_text`/`eval` work — but it **never
  executes the page's own `<script>` elements**. Nothing draws into the canvas buffers, so
  there is no drain to feed anywhere.
* `screenshot_cpu_rgba` (line 428) calls `render_to_image_cpu(…, &[], …)` — the image set is
  unconditionally **empty**. Even decoded `<img>` bitmaps are absent by design (documented in
  `snapshot_cpu.rs`: every image box paints the grey placeholder).

So two independent gaps stack: no script execution → no canvas pixels; empty image set → no
way to hand pixels to the rasterizer even if they existed.

## Ожидаемое

The deterministic CPU snapshot path should be able to cover scripted content: execute the
page's classic inline/external scripts the way the shell's `parse_and_layout` does, drain the
canvas updates, and pass the resulting image set (canvas bitmaps, and ideally decoded `<img>`
pixels) to `render_to_image_cpu`.

## Масштаб

* `cases::snapshot_cpu` cannot regress-test **any** JS-produced rendering, Canvas 2D included —
  the dedicated `57-canvas-2d` reference is a blank frame that will stay green through a total
  breakage of Canvas 2D.
* Same blindness for decoded images: `18-images` / `19-object-fit` references capture grey
  placeholders, not pixels.
* Regenerating the references after a fix will legitimately change every page whose meaning
  depends on scripts or images.

## Замечание к скоупу

Fixing this changes what the shared gate asserts for many pages at once (references would be
regenerated wholesale), so it is a deliberate, standalone task rather than a side effect of a
bug fix. The live-window pipeline (`graphic_tests/run.py`) is unaffected — it renders both
canvas content and images.
