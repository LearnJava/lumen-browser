# BUG-456 — Контекст `OffscreenCanvas.getContext('2d')` — отдельный шим на 16 членов вместо 59, без единого геттера, а `getImageData` отдаёт сырую строку транспорта

**Статус:** OPEN (симптом 3 исправлен 2026-08-30 — см. ниже)
**Компонент:** js (`crates/js/src/offscreen_canvas.rs` — литерал `this._2d_context`)
**Найден:** 2026-07-29 (P2), WPT-VENDOR-html-canvas — прогон среза `html/canvas/offscreen`

## Симптом

`OffscreenCanvas` реализует свой контекст 2D **независимо** от элементного, и он
беднее втрое:

```
O members=16   list=canvas,fillStyle,strokeStyle,lineWidth,globalAlpha,fillRect,clearRect,
                    strokeRect,beginPath,moveTo,lineTo,closePath,arc,fill,stroke,getImageData
O element-members=59
O proto=12                            (то есть прототип — Object.prototype, см. BUG-449)
```

Отсутствуют, в частности (проверено `typeof k[name] === 'undefined'`):

```
rect  roundRect  ellipse  arcTo  bezierCurveTo  quadraticCurveTo  clip
save  restore  reset  translate  setTransform
measureText  fillText  drawImage  putImageData  createLinearGradient
```

`save`/`restore` нет вовсе — то есть у OffscreenCanvas не просто расходятся две
копии состояния ([BUG-455](BUG-455-OPEN.md)), а стека состояний нет как такового.

## Симптом 2: четыре атрибута — сеттеры без геттеров

`fillStyle`, `strokeStyle`, `lineWidth`, `globalAlpha` объявлены только через
`set …(val)`. Чтение даёт `undefined`:

```
O fillStyle-get=undefined      (перед этим записали '#00ff00', заливка сработала)
```

## Симптом 3: транспорт натива протекает в скрипт страницы — ИСПРАВЛЕНО 2026-08-30

`getImageData` не принимала параметров и возвращала результат натива как есть:

```js
getImageData: () => _lumen_offscreen_canvas2d_get_image_data(canvasId),
```

```
O getImageData type=string  hasData=false  raw=20,20,00ff00ff00ff00ff00ff00ff00ff00ff00…
```

То есть страница получала **строку** `"{w},{h},{hex_rgba}"` вместо объекта
`ImageData`. Это и был доминирующий класс отказов среза: 200+ сабтестов падали
`Cannot read properties of undefined (reading '0')` — тесты делают
`getImageData(…).data[0]`. Элементный контекст ту же строку хотя бы разбирал
(`dom.rs`, до BUG-448), пусть и игнорируя прямоугольник ([BUG-448](BUG-448-FIXED.md)).

Пиксели при этом были верные (`00ff00ff` — ровно то, что залили): растеризатор
работал, был сломан шим.

**Исправлено тем же приёмом, что BUG-448** (двойник по описанию): прямоугольник
стал параметром отдельного натива `_lumen_offscreen_canvas2d_get_image_data_rect`,
переиспользующего тот же `Context2D::get_image_data_rect`
(`crates/engine/canvas/src/image_data.rs`) — старый нативный биндинг
`_lumen_offscreen_canvas2d_get_image_data` не тронут, его используют
`transferToImageBitmap`/`createImageBitmap`/внутренние снимки, которым нужен
весь буфер целиком в hex-формате, и сужать его на месте значило бы сломать все
эти вызовы. Шим `getImageData(sx, sy, sw, sh)` теперь: требует все 4 аргумента
(иначе `TypeError`), коэрсит их через `[EnforceRange] long` (не-конечное —
`TypeError`), бросает `IndexSizeError` на нулевые `sw`/`sh` и нормализует
отрицательные, возвращает `{width, height, data: Uint8ClampedArray, colorSpace:
'srgb'}`. Проба «до» правки нашла эти три сопутствующих дефекта к названному
одному. Тесты — `offscreen_canvas.rs::tests_v8::js_offscreen_get_image_data_*`
(3 шт).

## Данные WPT

Срез `html/canvas/offscreen`: **1033/1046 harness OK, 59/1265 сабтестов** (для
сравнения элементный срез — 486/2764). Крупнейшие классы:

| Сообщение | Файлов |
|---|---|
| `Cannot read properties of undefined (reading 'N')` — `getImageData(...).data` | ~200 (вся серия `2d.fillStyle.parse.*`) |
| `ctx.reset is not a function` | 23 |
| `ctx.rect is not a function` | 25 |
| `ctx.roundRect is not a function` | 12 |

## Направление починки

Не дописывать второй шим, а **переиспользовать первый**: контекст элементного
canvas и контекст OffscreenCanvas по спеке — почти один и тот же набор членов
(`CanvasRenderingContext2D` vs `OffscreenCanvasRenderingContext2D`; различия —
`canvas`, `commit`, отсутствие `drawFocusIfNeeded`/`scrollPathIntoView`). Сейчас
это две независимые реализации, и вторая отстала. Правильный порядок: сначала
общий прототип из [BUG-449](BUG-449-OPEN.md), затем оба контекста как два класса
над одной фабрикой членов, различающиеся списком нативов.

`getImageData` (симптом 3) исправлена отдельно и дёшево, до этого рефакторинга —
см. выше. Остаток (симптомы 1 и 2) по-прежнему ждёт общего прототипа.
