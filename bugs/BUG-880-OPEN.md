# BUG-880 — `createImageBitmap` не принимает `<canvas>` («unsupported source type»), интерфейсного объекта `ImageBitmap` нет

**Статус:** OPEN
**Заведён:** 2026-08-23 (WPT-RUN-6, срез 27 — живой замер, вариант `imagebitmap`)
**Область:** `crates/js/src/dom.rs` — реализация `createImageBitmap` (сообщение `createImageBitmap: unsupported source type`); глобального `ImageBitmap` в шиме нет (`'ImageBitmap' in window === false`)
**Владелец:** P1/P3 (`lumen-js`). Заведён P2 в ходе WPT-задачи, здесь не чинится.

## Симптом

`createImageBitmap(canvas)` отклоняется с `TypeError: createImageBitmap:
unsupported source type`, хотя `<canvas>` — один из основных
`ImageBitmapSource` (HTML LS §4.12.5.6). Рядом: `window.ImageBitmap` —
`undefined`, то есть `instanceof`-проверки и `'ImageBitmap' in self`
(типовая идиома WPT для детекта) не работают.

Что при этом есть и работает: сама функция `createImageBitmap`,
`OffscreenCanvas`, `canvas.transferControlToOffscreen`, контекст
`canvas.getContext('bitmaprenderer')`.

## Прямое измерение

`tests/wpt/verify_callback_import_preload_gaps.py --variant imagebitmap`
(2026-08-23, dev-release, Linux, `main` = `34cbefd25`):

```
ib-api createImageBitmap=function ImageBitmap=undefined
       OffscreenCanvas=function transferControlToOffscreen=function
ib-bitmaprenderer=ok
ib-rejected TypeError: createImageBitmap: unsupported source type
ib-checked
```

## Цена по WPT

* `imagebitmap-renderingcontext/bitmaprenderer-as-imagesource.html` — все три
  сабтеста строят `ImageBitmap` из canvas;
* `html/canvas/element/manual/imagebitmap/createImageBitmap-in-worker-transfer.html`
  — «Transfer ImageBitmap created in worker».

Обе категории не вендорены целиком, так что цена по остатку WPT-RUN-5 (2 id)
— нижняя граница.

## Что дальше

Список допустимых источников в спеке: `HTMLImageElement`, `SVGImageElement`,
`HTMLVideoElement`, `HTMLCanvasElement`, `Blob`, `ImageData`, `ImageBitmap`,
`OffscreenCanvas`. Дешёвый первый шаг — canvas и `ImageData` (пиксели уже
есть в движке) плюс интерфейсный объект `ImageBitmap` с `width`/`height`/
`close()`.
