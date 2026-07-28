# BUG-420 — Ни один интерфейс Canvas 2D не существует как глобальный объект; контекст, ImageData, TextMetrics, градиент и паттерн — обычные литералы

**Статус:** OPEN
**Компонент:** js (`crates/js/src/dom.rs` — `getContext` в фабрике обёрток `:5954-6014`,
`_lumen_make_canvas2d_ctx`; `crates/js/src/canvas2d.rs`)
**Найден:** 2026-07-28 (P2), WPT-VENDOR-html-canvas, проба `--dump-layout` + прогон

## Симптом

Проба (`--dump-layout`, разметочный `<canvas id=c>`, у которого `getContext('2d')`
работает):

```
CanvasRenderingContext2D:undefined  OffscreenCanvasRenderingContext2D:undefined
ImageData:undefined                 TextMetrics:undefined
CanvasGradient:undefined            CanvasPattern:undefined
ImageBitmap:undefined               ImageBitmapRenderingContext:undefined
DOMMatrix:undefined                 DOMMatrixReadOnly:undefined   DOMPoint:undefined
Path2D:function                     OffscreenCanvas:function      HTMLCanvasElement:function
```

Из 14 интерфейсов, которых требует HTML LS §4.12.5 (+ Geometry Interfaces), в глобальной
области есть три. Возвращаемые значения — объектные литералы, а не экземпляры:

| Выражение | Реально | По спеке |
|---|---|---|
| `ctx.constructor.name` | `Object` | `CanvasRenderingContext2D` |
| `Object.getPrototypeOf(ctx)` | `Object.prototype` | `CanvasRenderingContext2D.prototype` |
| `ctx.getImageData(0,0,1,1).constructor.name` | `Object` | `ImageData` |
| `ctx.measureText('x').constructor` | `Object` | `TextMetrics` |
| `ctx.createLinearGradient(…)` | `{__gid__, addColorStop}` | `CanvasGradient` |
| `ctx.createPattern(c,'repeat')` | `Object` | `CanvasPattern` |

Все ~48 членов контекста — **собственные** свойства каждого экземпляра
(`Object.prototype.hasOwnProperty.call(ctx,'fillRect') === true`), прототип пуст. Тот же
класс дефекта, что [BUG-367](BUG-367-OPEN.md) описывает для `Element`.

Данные при этом настоящие: `fillRect` + `getImageData` дают корректные пиксели
(`18,52,86,255` для `#123456`), `createImageData(2,2).data.length === 16`,
`ImageData.data` — честный `Uint8ClampedArray`. Сломана именно объектная модель, не
растеризация.

Побочные следствия, каждое видно отдельным сабтестом WPT:

- `instanceof` невозможен ни для одного результата (`ctx instanceof CanvasRenderingContext2D`
  бросает `ReferenceError`, а не даёт `false`);
- `new ImageData(w,h)` / `new ImageData(arr,w,h)` — конструктора нет вовсе, хотя
  спека делает `ImageData` конструируемым (единственный способ получить объект —
  `ctx.createImageData`/`getImageData`);
- `ImageData.colorSpace` отсутствует (`undefined`), как и весь `PredefinedColorSpace`;
- `TextMetrics` отдаёт 3 атрибута из 12: есть `width`,
  `actualBoundingBoxAscent`, `actualBoundingBoxDescent`; нет
  `actualBoundingBoxLeft/Right`, `fontBoundingBoxAscent/Descent`,
  `emHeightAscent/Descent`, `hangingBaseline`, `alphabeticBaseline`,
  `ideographicBaseline`;
- патч прототипа со страницы (обычный приём полифилов и самих тестов WPT —
  `CanvasRenderingContext2D.prototype.foo = …`) невозможен.

## Смежное: 14 членов контекста отсутствуют

Проверка `n in ctx` по списку членов `CanvasRenderingContext2D` из HTML LS даёт
отсутствующими:

```
getContextAttributes, reset, isContextLost, getTransform, imageSmoothingQuality,
drawFocusIfNeeded, scrollPathIntoView, letterSpacing, wordSpacing, fontKerning,
fontStretch, fontVariantCaps, textRendering, roundRect
```

`roundRect` — единственный из них, у которого есть собственный подкаталог тестов
(`element/path-objects/2d.path.roundrect.*`); остальные бьют по одному-двум id.

## Данные WPT

Срез `html/canvas` (`run_report.py --all --root html/canvas --recursive`,
детали и точные числа — в строке `WPT-VENDOR-html-canvas` ROADMAP.md). Показательные
файлы: `element/conformance-requirements/2d.conformance.requirements.basics.html`
(проверяет ровно существование интерфейсов и прототипов),
`offscreen/conformance-requirements/*`, весь `element/pixel-manipulation/*`
(`ImageData` как конструктор и как тип), `element/text/2d.text.measure.*`
(поля `TextMetrics`).

## Направление починки

Один приём закрывает всю группу: объявить интерфейсы классами в `WEB_API_SHIM`
(как уже сделано для `Path2D`/`OffscreenCanvas` — они единственные, что здесь целы),
перенести члены с экземпляра на `X.prototype`, а фабрики (`getContext`, `getImageData`,
`measureText`, `createLinearGradient`, `createPattern`) заставить возвращать
`Object.create(X.prototype)` с данными в скрытых полях. Нативы `_lumen_canvas2d_*`
трогать не нужно — они и так отдают правильные числа.
