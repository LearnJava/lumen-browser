# BUG-672 — `Serial`/`SerialPort` interfaces are directly constructible with `new`, though the spec defines no constructor

**Статус:** OPEN
**Компонент:** js (`crates/js/src/serial.rs` — `SERIAL_SHIM`)
**Найден:** P2, WPT-VENDOR-serial, 2026-08-06

## Симптом

Категория `serial` (`tests/wpt/serial/`, 18 файлов) — вендорена и прогнана целиком
(`run_report.py --all --root serial --recursive`, ~3:51, 10 отобранных id, 5
`*-manual.https.html` исключены собственным фильтром раннера): **0/10 harness
OK**. Все 10 — `.https.`, все TIMEOUT на уже задокументированном TLS-гэпе
`UnknownIssuer` (`network error: TLS handshake: invalid peer certificate:
UnknownIssuer`), ни один не дошёл до навигации.

Serial API реально реализован как Phase 0 заглушка (`crates/js/src/serial.rs`,
`navigator.serial.requestPort()`/`SerialPort.open()`/`.close()` намеренно
бросают `NotSupportedError`, `getPorts()` резолвится в `[]` — задокументировано
и соответствует комментарию в исходнике). Живая проба (`--mcp-live-port`) на
контейнере (`navigator.serial instanceof EventTarget === true`, прототипная
цепочка исправна — в отличие от класса BUG-664/BUG-668) нашла независимый
дефект в WebIDL-форме обоих интерфейсов:

```json
"SerialPort constructable (new SerialPort())?" -> "constructed-ok"
"Serial constructable (new Serial())?" -> "constructed-ok"
```

Оба интерфейса (`window.Serial`, `window.SerialPort`) реализованы как обычные
ES6 `class X extends EventTarget`, поэтому `new SerialPort()`/`new Serial()`
успешно конструируются с любой (или без) страницы, хотя апстримная спека
(WICG Serial API) не определяет конструктор ни для одного из них — оба
объекта существуют только как значения, отдаваемые движком (`navigator.serial`
— синглтон; `SerialPort` — элементы массива `getPorts()`/результат
`requestPort()`). По WebIDL интерфейс без операции-конструктора при вызове с
`new` обязан бросать `TypeError`.

## Масштаб

Тот же класс дефекта, что уже открыт для `Report`/`ReportingObserver`
([BUG-629](BUG-629-OPEN.md)) и `FileSystemFileHandle`
([BUG-374](BUG-374-FIXED.md)) — подделываемый объект, неотличимый через
`instanceof SerialPort`/`instanceof Serial` от настоящего, выданного
движком. Здесь — третья независимая поверхность того же системного паттерна
(`SERIAL_SHIM` не ставит guard на `new.target`/не блокирует конструктор).
Функциональный WPT-сигнал по категории отсутствует целиком (TLS-гэп режет
все 10 id до навигации), находка — только из живой пробы.

## Причина

Не установлена детально (вне скоупа WPT-VENDOR-задачи). `SERIAL_SHIM`
(`crates/js/src/serial.rs:16-67`) объявляет `class SerialPort extends
EventTarget { constructor() { ... } }` и `class Serial extends EventTarget
{ constructor() { ... } }` без проверки, что вызов пришёл из движка, а не со
страницы — обычный JS `class` конструктор общедоступен по определению, нужен
явный guard (символ-капча/`new.target` проверка/фабричная функция вместо
публичного класса), которого здесь нет.

## Дальше

Fix scope: заблокировать публичный `new SerialPort()`/`new Serial()` (guard
через непубличный символ, передаваемый из фабрики движка, либо превратить
конструктор в throw + внутреннюю фабричную функцию, как для других
`[Exposed]`-интерфейсов без конструктора). Не требует TLS-гэпа для
воспроизведения/фикса — живой `--mcp-live-port`-пробы достаточно для
верификации.
