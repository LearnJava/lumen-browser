# Задача: createImageBitmap + ImageBitmapRenderingContext

**Developer:** P1
**Ветка:** `p1-imagebitmap`
**Размер:** S
**Крейты:** `lumen-js`

## Goal

Дошить ImageBitmap-подсистему: реализовать `canvas.getContext('bitmaprenderer')`
(`ImageBitmapRenderingContext` + `transferFromImageBitmap`) и расширить
`createImageBitmap` на источники Blob/`<img>` (сейчас — только ImageData и
OffscreenCanvas). ImageBitmap должен переноситься на страничный `<canvas>` через
bitmaprenderer-контекст.

## Current state (сверено с кодом 2026-07-05)

PARTIAL: часть `createImageBitmap` есть, `ImageBitmapRenderingContext` —
отсутствует полностью.

- `crates/js/src/offscreen_canvas.rs:518` — `globalThis.createImageBitmap`:
  Promise-обёртка. Поддержаны источники:
  - **ImageData** (`offscreen_canvas.rs:527`): hex-кодит RGBA →
    `_lumen_offscreen_canvas_from_image_data` (`offscreen_canvas.rs:376`) →
    возвращает `{width,height,__canvas_id__,close}`.
  - **OffscreenCanvas** (`offscreen_canvas.rs:551`): снапшот через
    `_lumen_offscreen_canvas_transfer_to_image_bitmap` (`offscreen_canvas.rs:351`)
    → `{width,height,data,close}`.
  - **Blob** (`offscreen_canvas.rs:568`): `reject` — «requires image decoding
    (not yet implemented)».
  - **HTMLImageElement** (`offscreen_canvas.rs:574`): `reject` — «not yet implemented».
- `crates/js/src/offscreen_canvas.rs:489` — `OffscreenCanvas.transferToImageBitmap`
  реализован (возвращает `{width,height,data,close}`).
- **Нет** `ImageBitmapRenderingContext` / `bitmaprenderer` нигде в коде
  (grep `bitmaprenderer`/`ImageBitmapRenderingContext`/`transferFromImageBitmap`
  → только `ROADMAP.md` и roadmap-html, ни одного .rs). Значит
  `canvas.getContext('bitmaprenderer')` вернёт null, ImageBitmap некуда положить.
- Форма ImageBitmap **неконсистентна**: ImageData-путь отдаёт `__canvas_id__`
  (offscreen_canvas.rs:546), а OffscreenCanvas/transfer-путь — hex `data`
  (offscreen_canvas.rs:558). bitmaprenderer должен уметь оба.
- `crates/engine/image/src/lib.rs` — `decode()` умеет PNG/JPEG/WebP/GIF/AVIF —
  можно переиспользовать для Blob-источника (нужен мост JS Blob-байты → decode).

## Entry points

- `crates/js/src/offscreen_canvas.rs:518` — `createImageBitmap` (расширить источники).
- `crates/js/src/offscreen_canvas.rs:376` — `_lumen_offscreen_canvas_from_image_data` (образец натива RGBA→canvas_id).
- `crates/js/src/offscreen_canvas.rs:151` — `install_offscreen_canvas_bindings` (сюда добавлять bitmaprenderer + нативы декода Blob).
- `crates/js/src/dom.rs` — перехват `canvas.getContext(...)` (проверить, где регистрируется getContext для DOM-canvas, чтобы добавить ветку `'bitmaprenderer'`).
- `crates/engine/image/src/lib.rs::decode` — декод Blob-байтов в RGBA8.

## Срезы (декомпозиция)

### Срез 1 — XS — Унифицировать форму ImageBitmap
Свести оба пути `createImageBitmap` (`offscreen_canvas.rs:546` и `:558`) к
единой форме `{width, height, __canvas_id__, close()}` (canvas_id как источник
пикселей). Для OffscreenCanvas-снапшота — класть пиксели в новый offscreen через
`create_offscreen_from_pixels` (`offscreen_canvas.rs:113`) и возвращать его id.

### Срез 2 — S — `ImageBitmapRenderingContext` (`bitmaprenderer`)
Добавить в шим класс контекста с `transferFromImageBitmap(bitmap)` и
`canvas` back-reference (HTML Living Standard §4.12.5.1). Хранить последний
переданный bitmap (его `__canvas_id__`/пиксели). В `getContext` DOM-canvas
(`dom.rs`) добавить ветку `contextType === 'bitmaprenderer'`.

### Срез 3 — S — Present bitmap в страничный `<canvas>`
`transferFromImageBitmap` должен положить пиксели bitmap в бэкинг DOM-canvas,
чтобы они отрисовались в окне (по образцу того, как offscreen-canvas
композитится через `flush_dirty`, `offscreen_canvas.rs:127`). Нативный мост:
взять RGBA по `__canvas_id__` и записать в буфер экранного canvas-узла.

### Срез 4 — S — `createImageBitmap` из Blob
Заменить `reject` (`offscreen_canvas.rs:568`) на реальный путь: JS Blob →
байты (`source._bytes`, `offscreen_canvas.rs:568`) → новый натив
`_lumen_decode_image_to_canvas(bytes)` поверх `lumen_image::decode()` →
`__canvas_id__`. Учесть async/Promise-контракт.

### Срез 5 — XS — `createImageBitmap` из `<img>`
Если у `<img>`-узла доступны декодированные пиксели в движке — прокинуть их в
offscreen (`create_offscreen_from_pixels`). Если пиксели `<img>` из JS
недоступны — оставить `reject`, но пометить ограничение в DoD (не блокирует
основную задачу).

### Срез 6 — XS — Опции обрезки (sx, sy, sw, sh)
`createImageBitmap(source, sx, sy, sw, sh)` (`offscreen_canvas.rs:519`
принимает аргументы, но не использует). Реализовать crop при переносе в
offscreen (можно на JS-стороне до hex-кодирования).

## Tests

- Юнит `crates/js/src/offscreen_canvas.rs` (mod tests, `offscreen_canvas.rs:586`):
  `getContext('bitmaprenderer')` не null; `transferFromImageBitmap` кладёт
  пиксели (проверка через `flush_dirty`/чтение canvas-буфера).
- Юнит: `createImageBitmap(imageData)` возвращает bitmap с `__canvas_id__`
  (унификация формы, срез 1).
- graphic_test: новый `graphic_tests/NN-imagebitmap.html` (магента-рамка) —
  нарисовать ImageData → `createImageBitmap` → `bitmaprenderer` present на
  `<canvas>`; демо в `1000000-final.html`; `COVERAGE.md` + `TESTS` в `run.py`.

## Definition of done

- [ ] `canvas.getContext('bitmaprenderer')` возвращает `ImageBitmapRenderingContext`.
- [ ] `transferFromImageBitmap` переносит bitmap на страничный `<canvas>` (видно в окне).
- [ ] Форма ImageBitmap унифицирована (`__canvas_id__`) для всех источников.
- [ ] `createImageBitmap` из Blob декодирует через `lumen_image::decode` (PNG/JPEG/WebP/GIF/AVIF).
- [ ] Опции обрезки `sx/sy/sw/sh` работают.
- [ ] Ограничение по `<img>`-источнику (если пиксели недоступны) задокументировано.
- [ ] graphic_test `NN-imagebitmap` проходит (порог 0.5%).
- [ ] `CAPABILITIES.md` + `subsystems/js.md` обновлены (bitmaprenderer ✅/🟡).
