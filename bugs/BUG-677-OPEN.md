# BUG-677 — `FaceDetector`/`BarcodeDetector`/`TextDetector`: no `Symbol.toStringTag`, wrong constructor `.length`, internal state leaked as own instance properties

**Статус:** OPEN
**Компонент:** js (`crates/js/src/shape_detection.rs:18-86` — `SHAPE_DETECTION_SHIM`)
**Найден:** P2, WPT-VENDOR-shape-detection, 2026-08-06

## Симптом

Категория `shape-detection` (`tests/wpt/shape-detection/`, 26 файлов, 23
отобранных id) — вендорена и прогнана целиком (`run_report.py --all --root
shape-detection --recursive --processes=4`, 2 мин 16 с) — **0/23 harness OK,
0/0 сабтестов**. Все 23 id — `.https.`/`.sub.https.`, все TIMEOUT на уже
задокументированном TLS-гэпе `UnknownIssuer` (порт 18443) — ни один тест не
дошёл до навигации, прогон не дал прямого сигнала о самом API.

Shape Detection API реально реализован в движке как явная Phase 0 заглушка
(doc-комментарий `shape_detection.rs:1-4` точно описывает поведение:
`FaceDetector`/`BarcodeDetector`/`TextDetector` всегда возвращают пустой
массив, детекция не выполняется) — не баг сам по себе. Живая проба
(`--mcp-live-port`, локальная HTTP-страница со `<script>`, т.к. `about:blank`
не создаёт JS-контекст, а `data:`-URL не резолвится вовсе — тот же класс
дефекта, что [BUG-651](BUG-651-OPEN.md) для `file://`) нашла независимую
находку — три отклонения от WebIDL-формы интерфейса, общие для всех трёх
классов:

```json
{"faceLen":1,"barcodeLen":1,"textLen":1,
 "faceProtoTag":"[object Object]","barcodeProtoTag":"[object Object]","textProtoTag":"[object Object]",
 "faceOwnProps":["options","maxDetectedFaces"],
 "barcodeOwnProps":["options","formats"],
 "textOwnProps":["options"],
 "faceProtoProps":["constructor","detect"],
 "hasFaceToStringTag":false,"hasBarcodeToStringTag":false,"hasTextToStringTag":false,
 "callableWithoutNew":"TypeError: Class constructor FaceDetector cannot be invoked without 'new'"}
```

1. **`constructor.length` равен 1, должен быть 0.** Все три конструктора в
   спеке принимают `optional <X>DetectorOptions options = {}` — WebIDL для
   optional-аргумента со значением по умолчанию даёт `.length === 0`.
   Шим объявляет `constructor(options)` без `= {}`, поэтому
   `FaceDetector.length === 1` (аналогично `BarcodeDetector`/`TextDetector`)
   — код, проверяющий arity перед вызовом (частый паттерн в WPT
   idlharness-тестах), получает неверное значение.
2. **Нет `Symbol.toStringTag`.** `Object.prototype.toString.call(new
   FaceDetector())` возвращает `"[object Object]"` вместо `"[object
   FaceDetector]"` — то же самое для `BarcodeDetector`/`TextDetector`. Спека
   определяет каждый как отдельный WebIDL-интерфейс с собственным тегом;
   движок в других местах (`dom.rs:11218`, `es2026_proposals.rs:336,409`)
   расставляет `Symbol.toStringTag` явно там, где это сделано — здесь не
   сделано вовсе.
3. **Внутреннее состояние утекает как собственные перечисляемые свойства
   инстанса.** `this.options = options || {}` (все три класса) и
   `this.maxDetectedFaces`/`this.formats` (Face/Barcode) — ни одно из этих
   имён не входит в WebIDL-интерфейс ни одного из трёх типов; в спеке это
   внутренние слоты, не видимые через `Object.keys`/`for...in`/
   `JSON.stringify`. `idlharness.https.any.js` (единственный тест категории,
   проверяющий форму интерфейса вместо самой детекции) ожидает ровно
   спековый набор членов.

`callableWithoutNew` — не баг: класс-семантика ES6 уже корректно бросает
`TypeError` без `new` без отдельной логики в шиме.

## Причина

Тот же класс дефекта, что [BUG-365](BUG-365-FIXED.md) (`EyeDropper`): Phase 0
JS-шим написан как обычный ES6-класс без сверки с WebIDL-формой
(`Symbol.toStringTag`, arity через default-параметр, отсутствие лишних
собственных свойств инстанса) — функционально `detect()`/`getSupportedFormats()`
работают как документировано, но интерфейсная "оболочка" не совпадает со
спекой достаточно точно, чтобы idlharness-тесты проходили.

## Дальше

Fix scope в `crates/js/src/shape_detection.rs`:
* `constructor(options = {})` вместо `constructor(options)` на всех трёх
  классах — даёт `.length === 0`;
* `Object.defineProperty(<X>Detector.prototype, Symbol.toStringTag, {value:
  '<X>Detector', configurable: true})` на всех трёх (паттерн — `dom.rs:11218`);
* хранить `options`/`maxDetectedFaces`/`formats` в замыкании (WeakMap keyed
  by `this`, либо просто не сохранять — Phase 0 не читает их обратно нигде)
  вместо `this.<field> = …`, чтобы `Object.getOwnPropertyNames(instance)`
  был пуст.
