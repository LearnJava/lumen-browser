# BUG-395 — `getCurrentPosition`/`watchPosition` никогда не бросают `TypeError` на отсутствующий/невалидный success-колбэк

**Статус:** OPEN
**Компонент:** js (`crates/js/src/geolocation.rs:128-155` — `GEO_SHIM`,
функции `getCurrentPosition`/`watchPosition`)
**Найден:** P2, WPT-VENDOR-geolocation (2026-07-28), статический разбор
шима — тесты `getCurrentPosition_TypeError.https.html`/
`watchPosition_TypeError.https.html` вендорены и исполняются (не
testdriver-гейченные, не `.https.`-заблокированные другим классом), но
падают TIMEOUT из-за отдельного HTTPS-порт-гэпа этого прогона

## Симптом

По спеке (WebIDL `getCurrentPosition(PositionCallback successCallback, ...)`,
`successCallback` — обязательный аргумент) следующие вызовы обязаны
бросать `TypeError`:

```js
navigator.geolocation.getCurrentPosition();                 // нет аргументов
navigator.geolocation.getCurrentPosition(null);              // null вместо колбэка
navigator.geolocation.getCurrentPosition(3);                 // не-функция
navigator.geolocation.getCurrentPosition(() => {}, 4);       // невалидный error-колбэк
navigator.geolocation.getCurrentPosition({handleEvent: ()=>{}}); // legacy event handler object
```

Реальный шим (`GEO_SHIM`, `geolocation.rs:129-137`) ничего из этого не
бросает — вызов без аргументов или с `null`/`3`/`{}` вместо колбэка
просто тихо ничего не делает:

```js
getCurrentPosition: function(success, error) {
  if (_coords) {
    var pos = makePosition(_coords);
    _defer(function() { if (typeof success === 'function') success(pos); });
  } else {
    var err = permDenied();
    _defer(function() { if (typeof error === 'function') error(err); });
  }
}
```

`typeof success === 'function'` — это защита от собственного падения
шима (не вызывать не-функцию), а не спековая валидация: она не бросает
исключение, просто "success"-путь молча не срабатывает. Ровно тот же
паттерн у `watchPosition` (`:139-155`).

Это ломает три вендоренных теста напрямую:
`getCurrentPosition_TypeError.https.html` (7 assert'ов),
`watchPosition_TypeError.https.html` (аналогичный набор для
`watchPosition`) — оба не гейчены testdriver.js и не должны требовать
живого permission-промпта (тестируется бросок исключения синхронно, до
любого асинхронного шага), но в этом прогоне недостижимы одним уровнем
выше: категория целиком на `.https.`, и минимальный экзекьютор не
поднимает HTTPS-порт (тот же гэп, что у `WebCryptoAPI`/`ai`/
`ambient-light` и десятков других категорий, см. `tests/wpt/VENDOR.md`).
Находка получена чтением исходника шима, а не прогоном — тот же приём,
что дал BUG-393/394 на `generic-sensor` (недостижимый тест не значит
отсутствие дефекта).

## Причина

Стаб (`geolocation.rs:1-15`, собственный doc-comment: "Geolocation API
stub") реализовал только happy-path callback-инвокацию и никогда не
закладывал WebIDL-валидацию аргументов — ни числа, ни типа.
`clearWatch` в этом смысле корректен (спека и вендоренный
`clearWatch_TypeError.https.html` требуют ровно "не бросать" на любой
невалидный id — уже так и есть).

## Как чинить

В начале `getCurrentPosition`/`watchPosition` (JS-уровень шима,
`GEO_SHIM`) добавить синхронную проверку первого аргумента: бросать
`TypeError`, если `success` не передан или не является `function`
(включая объекты вида `{handleEvent}}` — WebIDL callback-интерфейсы не
принимают legacy event-handler объекты для `PositionCallback`). Второй
аргумент (`error`) при передаче должен быть либо `function`, либо
`null`/`undefined` — иначе тоже `TypeError`. Валидация должна быть
синхронной (бросок из самого вызова `getCurrentPosition(...)`, не из
отложенного колбэка), как ожидает `assert_throws_js` в обоих тестах.

Регрессия без WPT: `navigator.geolocation.getCurrentPosition()` бросает
`TypeError`, `navigator.geolocation.getCurrentPosition(()=>{}, 4)` бросает
`TypeError`, `navigator.geolocation.clearWatch(NaN)` по-прежнему не
бросает.

## Связанные

* Категория `geolocation` — скоуп ⬜ (кандидат, не аппаратный 🚫-класс),
  первая находка не от пробы "вне-скоуп API", а от чтения кода
  in-scope стаба.
* HTTPS-порт-гэп прогона — тот же класс, что у `WebCryptoAPI`/`ai`/
  `ambient-light`/`animation-worklet`/`attribution-reporting`/
  `audio-output`/`audio-session` (см. `tests/wpt/VENDOR.md`), не
  отдельный баг.
