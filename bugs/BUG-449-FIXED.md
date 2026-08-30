# BUG-449 — Ни один интерфейс Canvas 2D не существует как глобальный объект; контекст, ImageData, TextMetrics, градиент и паттерн — обычные литералы

**Статус:** FIXED 2026-08-30
**Компонент:** js (`crates/js/src/shim/web_api_shim_mid.js` — классы Canvas 2D и фабрика
контекста `_lumen_make_canvas2d_ctx`; натив метрик текста `crates/js/src/canvas2d.rs`)
**Найден:** 2026-07-28 (P2), WPT-VENDOR-html-canvas, проба `--dump-layout` + прогон
**Перепроверен:** 2026-07-29 на main после починки BUG-348 — воспроизводится без изменений
(номер сменён с BUG-420 на BUG-449: 420 занят чужим багом хрома, ветка отстала на 136 коммитов)

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

Все члены контекста — **собственные** свойства каждого экземпляра
(`Object.prototype.hasOwnProperty.call(ctx,'fillRect') === true`), а прототип — это
буквально `Object.prototype`. Замер 2026-07-29:
`Object.getOwnPropertyNames(ctx).length === 59`, у прототипа ровно 12 имён — столько
их у `Object.prototype`, то есть своего прототипа у контекста нет вовсе. Тот же
класс дефекта, что [BUG-367](BUG-367-FIXED.md) описывает для `Element`.

Данные при этом настоящие: `fillRect` + `getImageData` дают корректные пиксели
(`18,52,86,255` для `#123456`), `createImageData(2,2).data.length === 16`,
`ImageData.data` — честный `Uint8ClampedArray`. Сломана именно объектная модель, не
растеризация.

## Что починено 2026-08-30

Пять интерфейсов Canvas 2D — `CanvasRenderingContext2D`, `ImageData`, `TextMetrics`,
`CanvasGradient`, `CanvasPattern` — объявлены классами в шиме; члены живут на
прототипах, фабрики (`getContext`, `getImageData`/`createImageData`, `measureText`,
`create*Gradient`, `createPattern`) отдают `Object.create(X.prototype)`, а всё
состояние экземпляра лежит в ОДНОМ ненумеруемом слоте (`__canvas2d__`,
`__image_data__`, `__text_metrics__`, `__gid__`, `__patid__`).

**Слот — он же бренд-проверка, и это не украшение, а половина требований спеки.**
`CanvasRenderingContext2D.prototype.createImageData.call(null, …)` обязан бросать
`TypeError` (`2d.imageData.create1.this`), а `putImageData({width:1,height:1,data:[…]})`
— отвергать литерал, похожий на `ImageData` (`2d.imageData.put.wrongtype`); до правки
первый рисовал бы по nid, подсмотренному у чужого объекта, а второй уезжал в натив.

`Symbol.toStringTag` расставлен **по классу**, включая `Path2D`: один тег на общем
предке заставил бы подкласс называться именем базы — форма [BUG-912](BUG-912-OPEN.md).

### `new ImageData(…)` — по таблице ошибок спеки, а не «конструктора нет»

Конструктор реализует обе перегрузки WebIDL, и тип ошибки в нём — не косметика, а
свидетельство того, какую перегрузку выбрал разбор аргументов:

* аргумент 0 типа `Uint8ClampedArray` выбирает форму `(data, sw, sh)`, **что угодно
  другое** проваливается в `(sw, sh)` — поэтому `new ImageData(new Uint8Array(100), 25)`
  даёт `IndexSizeError` за нулевую ширину (`ToUint32(объект)` = 0), а не `TypeError` за
  чужой тип буфера;
* словарь `ImageDataSettings` конвертируется **до** шагов конструктора, поэтому
  `new ImageData(self, 4, 4)` — `TypeError` (третий аргумент число, а не объект), тогда
  как `new ImageData('width','height')` — `IndexSizeError`;
* длина буфера не кратна 4 → `InvalidStateError`; не кратна `4 × sw`, либо заданная
  высота не сходится с производной → `IndexSizeError`; буфер **используется**, а не
  копируется.

`width`/`height`/`data` стали readonly-геттерами (`2d.imageData.object.readonly`),
добавлен `pixelFormat`; запрос `rgba-float16` отвергается вслух, а не подменяется
восьмибитным буфером под именем плавающего.

### TextMetrics: двенадцать чисел из шрифта, а не из размера шрифта

Заявка называла «3 атрибута из 12». Добавить недостающие девять как `0.8 × размер` было
бы вторым дефектом того же рода, поэтому появился натив
`_lumen_canvas2d_text_metrics(nid, text) -> [12 чисел]` (`canvas2d.rs`): горизонтальные
границы считаются по **bbox глифов** (пустой глиф — пробел — в чернильный бокс не
входит, только в перо), вертикальные — по `hhea`, em-квадрат — по доле восходящей части.
Всё измеряется от **точки выравнивания** (`textAlign`) и от линии `textBaseline`, как
и просит §4.12.5.1.13, а таблица базовых линий — та же самая, по которой
`render_text_to_canvas` реально рисует: метрики обязаны описывать тот текст, который
рисуется.

### Смежное: члены контекста, которых не было

Добавлены `isContextLost()`, `getContextAttributes()`, `reset()` (§4.12.5.1.2 — битмап в
прозрачно-чёрный, состояние в начальные значения), `roundRect()` (единственный из
списка со своим подкаталогом тестов; углы — четверти эллипса, общий коэффициент сжатия,
отрицательные ширина/высота зеркалят прямоугольник вместе с углами) и семь свойств
состояния: `imageSmoothingQuality`, `letterSpacing`, `wordSpacing`, `fontKerning`,
`fontStretch`, `fontVariantCaps`, `textRendering` — значение вне перечисления
игнорируется, как требует §4.12.5.1.12, а не бросает.

### Дефект, которого заявка не называла

`ctx.setTransform()` **без аргументов** должен сбрасывать матрицу к единичной, а
отправлял в натив шесть `+undefined`, то есть шесть `NaN`. Найден чтением ассертов
`2d.conformance.requirements.basics`, который вызывает именно эту форму. Заодно
добавлена одноаргументная форма (`DOMMatrix2DInit`-словарь), а
`CanvasGradient.addColorStop` теперь отвергает смещение вне `[0, 1]`
(`IndexSizeError`) и нечисло (`TypeError`).

## Измерение

A/B по вендоренной категории `html/canvas/element` (1 496 файлов, ~16 мин на прогон),
все бинарники `dev-release` из одного дерева:

| | harness OK | сабтесты |
|---|---|---|
| main | 1 270/1 496 | 722/3 058 |
| срез 1 (объектная модель) | 1 270/1 496 | 754/2 851 |
| срез 1–3 (всё) | 1 278/1 496 | **838/3 059** |

Ни один файл не потерял зелёного сабтеста; сверх счётчика — семь reftest-ов
`reset/*` (`after-rasterization`, `drop_shadow`, `global_composite_operation`, `line`,
`misc`, `miter_limit`, `text`) перешли FAIL → PASS, а все 17 `path-objects/2d.path.roundrect.*`
— 0/1 → 1/1.

Регрессий нет. Четыре файла, у которых статус упал, — это падение wgpu
(`Error in Surface::present: Validation Error`, [BUG-453](BUG-453-OPEN.md)): оно
случается ровно дважды в **каждом** прогоне и бьёт по разным тестам, поэтому в базовом
прогоне убитым оказался `2d.pattern.image.broken` (и он же «улучшился» после правки), а
в проверочном — `2d.pattern.crosscanvas`. Счётчик «live window closed before replying» у
базового прогона даже выше (13 против 10). Падение общего числа собранных сабтестов
(3 058 → 2 851) целиком объясняется одним из этих падений: `canvas-display-p3-drawImage-`
`ImageBitmap-Blob` успевал собрать 208 сабтестов (0 зелёных) и не успел в другом прогоне.

Регрессионные тесты — `crates/js/src/dom/tests/v8_core/canvas_object_model.rs`
(21 тест: глобальные интерфейсы, прототипы, class string, патч прототипа, бренд-проверки,
таблица ошибок конструктора `ImageData`, readonly-атрибуты, `putImageData` против
литерала, метрики из шрифта и от точки выравнивания, `reset`, `roundRect`,
перечислимые свойства, `setTransform()`). Отдельный файл, а не хвост
`selectors_canvas_window.rs`: тот уже 1 874 строки при потолке 2 000.

## Остаток

* **`getTransform()`** не добавлен: он обязан вернуть `DOMMatrix`, а геометрических
  интерфейсов в движке нет вовсе ([BUG-522](BUG-522-OPEN.md)). По той же причине
  `CanvasPattern.setTransform` принимает `DOMMatrix2DInit`-словарь, запоминает его и
  **не применяет** — у нативного паттерна нет слота под матрицу.
* `drawFocusIfNeeded` / `scrollPathIntoView` не добавлены: единственное их наблюдаемое
  действие — рисование кольца фокуса и скролл, а пустой метод под этими именами был бы
  тем же молчаливым враньём, из-за которого заведён этот баг.
* `ImageBitmap`, `ImageBitmapRenderingContext`, `OffscreenCanvasRenderingContext2D` —
  по-прежнему литералы. У `OffscreenCanvas` свой шим (`offscreen_canvas.rs`), и его
  `getImageData()` вовсе не принимает аргументов — это [BUG-456](BUG-456-OPEN.md);
  `ImageBitmap` живёт в отдельном шиме `{width, height, __canvas_id__, close()}`.
* Члены `HTMLCanvasElement` по-прежнему стоят на **каждом** элементе DOM
  (`'getContext' in div` → `true`) — [BUG-450](BUG-450-OPEN.md), другая сторона той же
  фабрики обёрток.
* `letterSpacing`/`wordSpacing` и пять перечислимых свойств хранятся и отдаются, но
  растеризатор их не читает.

## Данные WPT

Срез `html/canvas` (`run_report.py --all --root html/canvas --recursive`,
детали и точные числа — в строке `WPT-VENDOR-html-canvas` ROADMAP.md). Показательные
файлы: `element/conformance-requirements/2d.conformance.requirements.basics.html`
(проверяет ровно существование интерфейсов и прототипов),
`offscreen/conformance-requirements/*`, весь `element/pixel-manipulation/*`
(`ImageData` как конструктор и как тип), `element/text/2d.text.measure.*`
(поля `TextMetrics`).
