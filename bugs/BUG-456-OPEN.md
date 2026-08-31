# BUG-456 — Контекст `OffscreenCanvas.getContext('2d')` — отдельный шим на 16 членов вместо 59, без единого геттера, а `getImageData` отдаёт сырую строку транспорта

**Статус:** OPEN (симптомы 2 и 3 исправлены, симптом 1 частично закрыт 2026-08-31 — см. ниже)
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

## Симптом 2: четыре атрибута — сеттеры без геттеров — ИСПРАВЛЕНО 2026-08-31

`fillStyle`, `strokeStyle`, `lineWidth`, `globalAlpha` объявлены только через
`set …(val)`. Чтение даёт `undefined`:

```
O fillStyle-get=undefined      (перед этим записали '#00ff00', заливка сработала)
```

**Проба «до» правки нашла ровно половину этого списка уже исправленной по
дороге**: `fillStyle`/`strokeStyle` к этому моменту (за счёт несвязанной
работы над BUG-451, канонической сериализацией цвета) уже имели рабочую пару
`get`/`set` — только `lineWidth` и `globalAlpha` оставались сеттерами без
геттера. Причина, почему литерал вообще мог содержать оба члена дважды:
`this._2d_context = { lineWidth: 1, globalAlpha: 1, …, set lineWidth(w) {…},
set globalAlpha(a) {…} }` — объектный литерал не запрещает повторное имя
ключа, последнее определение (аксессор) молча вытесняет плоское поле, так что
`lineWidth: 1` было мёртвым кодом с самого начала. Оба атрибута получили
собственные приватные переменные (`_lineWidth`/`_globalAlpha`, тот же приём,
что уже применён к `_fillStyle`/`_strokeStyle`) и пару `get`/`set`.

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

## Направление починки — почему «переиспользовать первый прототип» не сработало

Первоначальный план («сначала общий прототип из [BUG-449](BUG-449-FIXED.md),
затем оба контекста как два класса над одной фабрикой членов») разбился о
границу, которую сам BUG-449 не пересекает: `CanvasRenderingContext2D` —
класс в `web_api_shim_mid.js`, который эволюционирует в `dom.rs`'s
`WEB_API_SHIM` — **странице-only** шиме. `OffscreenCanvas` обязан работать и
в воркере (это весь смысл интерфейса — 2D-рисование без DOM), а воркер
получает `OFFSCREEN_CANVAS_SHIM` как отдельный `rt.eval` (`worker.rs`) без
`WEB_API_SHIM_MID` вообще — так же, как `xhr.rs`/`audio_element.rs` не видят
исправлений в `dom.rs` (BUG-780 lesson, тот же список гочей CLAUDE.md).
Наследование от `CanvasRenderingContext2D.prototype` в шиме, который эта
страница не гарантированно грузит, дало бы `ReferenceError` в воркере вместо
объекта — второй, более тихий вариант того же дефекта.

**Что сделано вместо этого (2026-08-31): второй шим не переписан целиком, а
достроен своей собственной, независимой копией того же алгоритма**, с
нативами, зовущими тот же `lumen_canvas::Context2D` (одна инженерная модель
на обе реализации, просто два JS-фасада над ней — двумя разными регистрами,
`CANVASES`/`nid` на элементной стороне и `OFFSCREEN_CANVASES`/`canvas_id` на
offscreen). Добавлено 13 новых нативов
(`_lumen_offscreen_canvas2d_{save,restore,translate,rotate,scale,transform,
set_transform,reset_transform,rect,bezier_curve_to,quadratic_curve_to,arc_to,
clip}`), каждый — тонкая обёртка над уже существующим публичным методом
`Context2D` (движок их уже реализовывал для элементного пути, здесь просто не
было JS-биндинга). `ellipse`/`roundRect` не получили отдельных нативов —
ни у элементного контекста их нет, оба композируются из
`save`/`translate`/`rotate`/`scale`/`arc`/`restore` в JS (`roundRect` —
дословный порт `_lumen_corner_radius`/`roundRect` из `web_api_shim_mid.js`,
адаптированный на offscreen-имена нативов, раз шимы не делят JS-реалм).

Закрыта и часть государственного стека: `save`/`restore` теперь существуют и
синхронизируют нативный стек (CTM/путь/клип) с четырьмя JS-зеркалируемыми
атрибутами (`fillStyle`/`strokeStyle`/`lineWidth`/`globalAlpha`) — тот же урок
BUG-455, что и у элементного контекста: если копии не двигаются в одном такте,
`restore()` возвращает страницу в состояние, которого JS-сторона не видит.
`reset()` (§4.12.5.1.2) тоже добавлен: сброс трансформации, полная заливка
прозрачным чёрным, обнуление пути и стека, возврат четырёх атрибутов к
дефолтам.

**Ещё не тронуто (остаток симптома 1):** `measureText`, `fillText`,
`strokeText`, `drawImage`, `putImageData`, `createImageData`,
`createLinearGradient`/`createRadialGradient`/`createConicGradient`,
`createPattern`, `isPointInPath`/`isPointInStroke`, `setLineDash`/
`getLineDash`, и свойства состояния `globalCompositeOperation`/`lineCap`/
`lineJoin`/`miterLimit`/`shadowColor`/`shadowBlur`/`shadowOffsetX`/
`shadowOffsetY`/`font`/`textAlign`/`textBaseline` — каждое требует либо нового
натива не тривиальной формы (текст, изображения, градиенты держат
собственное состояние), либо решения о том, где хранить дополнительные
JS-зеркалируемые атрибуты в `save`/`restore`. Следующий срез должен начать с
`globalCompositeOperation`/`lineCap`/`lineJoin`/`miterLimit`/`shadow*` —
это чистые сеттеры поверх уже публичных полей `Context2D`, симметричные
только что добавленным геометрическим нативам.

Тесты — `offscreen_canvas.rs::tests_v8::js_offscreen_{line_width_and_global_
alpha_round_trip, save_restore_round_trips_tracked_attributes, context_has_
transform_and_path_methods, transform_ops_do_not_throw, reset_clears_fill_
style_and_transform}` (5 шт).
