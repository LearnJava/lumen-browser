# BUG-664 — `NetworkInformation` (`navigator.connection`) не наследуется от `EventTarget`, `change`-событие никогда не доставляется

**Статус:** OPEN
**Компонент:** js (`crates/js/src/dom.rs:12855-12874` — IIFE секции «Network
Information API», `function NetworkInformation() { ... }`)
**Найден:** P2, WPT-VENDOR-savedata (2026-08-05), прямая проба
`--mcp-live-port`/`eval` (сам `idlharness.any.js` категории `savedata`
недостижим прогоном — HTTP 404 на невендоренные общие `/resources/
WebIDLParser.js`+`idlharness.js`, тот же инфра-гэп, что и у прочих
idlharness-категорий)

## Симптом

```js
var c = navigator.connection;
c instanceof EventTarget                                   // → false
typeof c.dispatchEvent                                     // → "undefined"
Object.getPrototypeOf(Object.getPrototypeOf(c)) === EventTarget.prototype
                                                             // → false
Object.getOwnPropertyNames(Object.getPrototypeOf(c))
                                                             // → ["constructor", "onchange", "addEventListener", "removeEventListener"]
Object.prototype.toString.call(c)                           // → "[object Object]" (спека требует "[object NetworkInformation]")
```

По W3C Network Information API §7, `interface NetworkInformation :
EventTarget` — `navigator.connection` обязан быть настоящим
`EventTarget`: `instanceof EventTarget` истинно, `dispatchEvent`
унаследован от глобального прототипа, `addEventListener`/
`removeEventListener` — та же реализация, что у остальных объектов
платформы (поддержка `once`/`capture`/реальных `Event`-объектов), и
`change`-событие обязано доставляться при изменении сетевых условий.

`idlharness.any.js` категории `savedata` (единственный тест) проверяет
именно это (`idl_array.add_objects({ NetworkInformation:
['navigator.connection'] })`), но сам тест недостижим прогоном —
`/resources/WebIDLParser.js` и `/resources/idlharness.js` не вендорятся
по устоявшейся конвенции (общие `/resources/*` хелперы вне текущей
категории). Находка получена прямой пробой, не прогоном.

## Причина

`dom.rs:12855-12863` — `NetworkInformation` собран как обычная функция-
конструктор с плоскими полями (`effectiveType`/`downlink`/`rtt`/
`saveData`/`type`/`_onchange`) вместо наследования от глобального
`EventTarget` (определён в этом же файле, `dom.rs:384`, и уже
используется десятками других шимов). `onchange` — accessor-свойство
поверх приватного `_onchange` (`:12864-12868`), а `addEventListener`/
`removeEventListener` (`:12869-12870`) — пустые функции-заглушки:

```js
NetworkInformation.prototype.addEventListener    = function() {};
NetworkInformation.prototype.removeEventListener = function() {};
```

То есть подписка молча не срабатывает независимо от способа (ни через
`onchange =`, ни через `addEventListener('change', ...)`) — нигде в
кодовой базе нет места, которое вызывало бы `dispatchEvent`/`_onchange`
на этом объекте, поэтому `change`-событие не доставляется в принципе,
даже если бы сама метрика сети умела меняться. Это не только IDL-
несоответствие (`instanceof`), но и функциональный пробел: скрипт,
слушающий `navigator.connection.onchange`, никогда не получит
уведомление.

Тот же класс дефекта, что [BUG-386](BUG-386-FIXED.md) (`PermissionStatus`),
[BUG-394](BUG-394-OPEN.md) (`Sensor`) и [BUG-400](BUG-400-OPEN.md)
(`performance`) — самодельные объектные литералы/мини-классы вместо
наследования от уже существующего глобального `EventTarget`.

## Как чинить

1. Заменить ручные `addEventListener`/`removeEventListener`-заглушки на
   `NetworkInformation.prototype = Object.create(EventTarget.prototype)`
   (или эквивалент, уже применённый в других недавно исправленных шимах
   этого файла), убрать домашние методы.
2. Установить `Symbol.toStringTag` на прототипе (`'NetworkInformation'`),
   раз объект собран вручную, а не через движковую WebIDL-обвязку.
3. Отдельный вопрос (не блокирует этот фикс) — реальное измерение сети в
   Lumen отсутствует (Phase 1 stub, статичные значения), так что
   `dispatchEvent` пока некому будет вызывать; фикс здесь ограничивается
   формой интерфейса, а не добавлением живой доставки событий.

Регрессия без WPT: `navigator.connection instanceof EventTarget === true`,
`Object.prototype.toString.call(navigator.connection) ===
'[object NetworkInformation]'`.

## Связанные

* [BUG-641](BUG-641-OPEN.md) — тот же шим (`dom.rs:12855-12874`), другая
  находка: `downlinkMax` отсутствует целиком.
* [BUG-386](BUG-386-FIXED.md) / [BUG-394](BUG-394-OPEN.md) /
  [BUG-400](BUG-400-OPEN.md) — тот же класс дефекта (не-`EventTarget`
  самодельные шимы) на других объектах.
