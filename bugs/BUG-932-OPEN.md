# BUG-932 — `OffscreenCanvas` 2D-контекст и его вспомогательные объекты — duck-typed литералы, не классы

**Статус:** OPEN
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
