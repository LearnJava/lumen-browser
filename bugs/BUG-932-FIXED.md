# BUG-932 — `OffscreenCanvas` 2D-контекст и его вспомогательные объекты — duck-typed литералы, не классы

**Статус:** FIXED 2026-08-31 (P3), кроме `ImageBitmap` — вынесен в [BUG-933](BUG-933-OPEN.md)
**Компонент:** js (`crates/js/src/offscreen_canvas.rs` — `_2d_context`/`OffscreenCanvas`/`ImageBitmap` объявлены объектными литералами)
**Найден:** 2026-08-31 (P3), при закрытии [BUG-456](BUG-456-FIXED.md)

## Симптом

[BUG-456](BUG-456-FIXED.md) довёл `OffscreenCanvas.getContext('2d')` до
функциональной полноты (member-параметр наравне с элементным контекстом,
рабочие геттеры, `getImageData`/`putImageData` на реальных прямоугольниках,
текст/изображения/градиенты/паттерны). Но сам контекст, а также объекты,
которые он производит, остаются **объектными литералами**, а не экземплярами
классов:

```
ctx.constructor.name === 'Object'
ctx instanceof OffscreenCanvasRenderingContext2D   // ReferenceError — глобала нет
```

То же верно для `CanvasGradient`/`CanvasPattern`/`TextMetrics` этой стороны
(duck-typed по `__gid__`/`__patid__`/полям, не `instanceof`-проверяемые) и для
значений, которые `getImageData()`/`createImageData()` возвращают (плоский
`{width, height, data, colorSpace}`, не `ImageData`).

Практические следствия:

- `instanceof` на любом из этих типов либо `false`, либо бросает
  `ReferenceError` (глобала не существует).
- Патч прототипа со страницы/воркера (приём полифилов и части тестов WPT)
  невозможен — патчить нечего.
- `Symbol.toStringTag`/`Object.prototype.toString.call(ctx)` не отдают
  спека-корректное имя интерфейса.

Данные (пиксели, вычисленные значения) при этом верны на всех путях,
измеренных срезами BUG-456 — дефект чисто в объектной модели, не в
растеризации.

## Почему не закрыт тем же способом, что BUG-449

[BUG-449](BUG-449-FIXED.md) закрыл ровно этот класс дефекта для элементного
контекста (`CanvasRenderingContext2D`/`ImageData`/`TextMetrics`/градиент/
паттерн стали классами с прототипами), но само это решение **не пересекает
границу**, которая стоит между двумя реализациями Canvas 2D в этом движке:
`CanvasRenderingContext2D` — класс в `web_api_shim_mid.js`, который эволюция
`WEB_API_SHIM` — **странице-only** шиме, устанавливаемом `dom.rs::install_dom`.

`OffscreenCanvas` обязан работать и в воркере (в этом весь смысл интерфейса —
2D-рисование без DOM), а воркер получает `OFFSCREEN_CANVAS_SHIM` как
отдельный `rt.eval` (`worker.rs`) без `WEB_API_SHIM_MID` вообще — тот же
список гочей CLAUDE.md, что `xhr.rs`/`audio_element.rs` не видят исправлений
в `dom.rs` (BUG-780 lesson). Наследование от `CanvasRenderingContext2D.prototype`
дало бы `ReferenceError` в воркере вместо объекта.

## Направление починки

Свой, независимый набор классов внутри `offscreen_canvas.rs` (или общий
JS-модуль текстом, который оба `rt.eval`-я — страница и воркер — включают
через `include_str!`, аналогично тому, как `crates/js/src/shim/*.js` уже
устроены с SPLIT-JS3) — не одна строка, отдельный объём работы:

- `OffscreenCanvasRenderingContext2D`, `CanvasGradient`, `CanvasPattern`,
  `TextMetrics`, `ImageData` как классы с прототипами, тем же приёмом, что
  BUG-449 применил к элементной стороне (`Object.create(X.prototype)`,
  ненумеруемый слот состояния как бренд-проверка, `Symbol.toStringTag`).
- `ImageBitmap` (упомянут в [BUG-449](BUG-449-FIXED.md)'s «Остаток» как тоже
  литерал) — тот же приём, тот же файл.
- Не копировать код класса из `web_api_shim_mid.js` буквально — воркер не
  видит эту строку; либо вынести общий шаблон класса в третий файл, который
  оба `rt.eval`-я конкатенируют, либо написать offscreen-версию заново по
  тому же образцу (как уже сделано для геометрии/текста в BUG-456).

## Данные

Не собраны отдельно от WPT-среза BUG-456 (`html/canvas/offscreen`,
1033/1046 harness OK на момент заведения) — этот дефект не был доминирующим
классом отказов там (`Cannot read properties of undefined` — BUG-456's
симптом 3 — был), но часть тестов на `instanceof`/`constructor.name`/
прототип-патчинг в этой категории по-прежнему падает по этой причине.

## Исправление (P3, 2026-08-31)

`OffscreenCanvasRenderingContext2D`/`CanvasGradient`/`CanvasPattern`/
`TextMetrics`/`ImageData` теперь реальные классы, не duck-typed литералы.

**`OffscreenCanvasRenderingContext2D`** — своя, независимая реализация
(`function OffscreenCanvasRenderingContext2D() { throw new TypeError('Illegal
constructor'); }`, `Symbol.toStringTag`), как и предполагало «Направление
починки» — интерфейс не пересекает границу страница/воркер. `getContext('2d')`
собирает состояние в `impl` тем же самым литералом, что и раньше (ни одна из
~40 сигнатур методов/аксессоров не менялась — методы не читают `this`,
поведенческий риск нулевой), а затем один раз пере-домашивает его:
`this._2d_context = Object.create(OffscreenCanvasRenderingContext2D.prototype,
Object.getOwnPropertyDescriptors(impl))`. `Object.getOwnPropertyDescriptors`
корректно переносит пары `get`/`set` как настоящие аксессоры, а не как
одноразовое чтение значения (чем грозил бы наивный `Object.assign`).

**Известное ограничение:** методы остаются собственными свойствами
экземпляра (не переехали на прототип целиком, в отличие от BUG-449's полного
переноса для элементного контекста) — патч страницей УЖЕ РЕАЛИЗОВАННОГО
метода через `OffscreenCanvasRenderingContext2D.prototype.fillRect = …` не
подействует (own-property экземпляра затенит прототип). Патч НЕреализованного
члена — работает (ничего не затеняет). `instanceof`/`constructor.name`/
`Symbol.toStringTag` работают полностью, для чего дефект и заведён.

**`CanvasGradient`/`CanvasPattern`/`TextMetrics`/`ImageData`** — типовой
guard-and-reuse: этот модуль эволюирует ПОСЛЕ `WEB_API_SHIM` в реальном
браузере (`v8_runtime.rs::install_dom` зовёт
`install_offscreen_canvas_bindings_v8` после `WEB_API_SHIM`, в той же
странице-реалм) — при наличии страничного класса (`web_api_shim_mid.js`,
BUG-449) используется ОН ЖЕ (`typeof globalThis.CanvasGradient === 'function' ?
globalThis.CanvasGradient : …`), так что
`offscreenCtx.createLinearGradient(...) instanceof CanvasGradient` совпадает с
элементным контекстом. При отсутствии странички класса (изолированный тест —
`tests_v8::with_offscreen` не ставит `WEB_API_SHIM`, а в будущем — воркер,
если/когда `OffscreenCanvas` туда доедет) — локальный самодостаточный
фолбэк, определённый тут же, тем же приёмом что BUG-780 (typeof-guard, а не
прямой вызов). Это устойчивее к границе страница/воркер, о которой
предупреждает «Почему не закрыт тем же способом»: воркер получит
РАБОТАЮЩИЙ локальный класс вместо `ReferenceError`, а не деградацию.

`getImageData`/`createImageData` минтят `ImageData` через
`_offscreen_make_image_data` вместо плоского литерала; `putImageData`
не менялась — она уже читала `.data`/`.width`/`.height` дак-тайпингом,
что одинаково работает и для литерала, и для настоящего экземпляра.

**Не сделано — остаток вынесен в [BUG-933](BUG-933-OPEN.md):** `ImageBitmap`
(упомянут в «Направлении починки» как тот же приём, тот же файл) остался
литералом `{width, height, __canvas_id__, close}`. Причина отделения: в
отличие от контекста/градиента/паттерна/метрик, `__canvas_id__` читается
как собственное перечислимое поле в ЧЕТЫРЁХ файлах (`canvas2d.rs`,
`offscreen_canvas.rs`, `worker.rs`, `web_api_shim_mid.js`) как приём
дак-тайпинга «это canvas-подобный объект» — превращение в класс требует
либо держать `__canvas_id__` геттером на прототипе (не тестировано против
всех четырёх потребителей, включая `worker.rs`'s structured-clone), либо
трогать код передачи между потоками. Больший радиус поражения, чем
оправдан этим срезом.

**Верификация**

1. `cargo test -p lumen-js --features v8-backend offscreen_canvas` — 52/52,
   включая 2 новых: `js_offscreen_canvas_context_is_real_class_instance`
   (constructor.name/instanceof/toStringTag на всех пяти классах разом) и
   `js_offscreen_canvas_context_illegal_constructor_throws` (`new
   OffscreenCanvasRenderingContext2D()` бросает `TypeError`, как остальные
   IDL-интерфейсы этого движка).
2. `cargo clippy -p lumen-js --features v8-backend --all-targets -- -D
   warnings` — чисто.
3. Все 50 ранее существовавших тестов offscreen_canvas прошли без изменений
   — литерал `impl` не менялся, только домашивание после сборки.
