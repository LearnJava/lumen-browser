# BUG-718 — `BroadcastChannel.postMessage` clones via `JSON.stringify` instead of structured clone

**Статус:** OPEN
**Компонент:** js (`crates/js/src/broadcast_channel.rs:193-219` — `BroadcastChannel.prototype.postMessage`)
**Найден:** P2, WPT-VENDOR-webmessaging, 2026-08-09

## Симптом

Тот же прогон, что и [BUG-717](BUG-717-OPEN.md)
(`webmessaging`, 136 id, 77/136 harness OK, 82/206 сабтестов).
`broadcastchannel/interface.any.html` (10/13 passed) даёт три сабтеста,
не объяснимых ни TLS-гэпом, ни отсутствием browsing context:

1. **`postMessage()` без аргумента не бросает.** WebIDL BroadcastChannel
   `postMessage(any message)` — `message` обязательный аргумент; вызов
   без него должен бросить `TypeError` ("1 argument required, but only 0
   present" — тот же паттерн, что уже правильно реализован для
   конструктора `BroadcastChannel(name)` тремя строками выше в том же
   файле, `broadcast_channel.rs:194-196`). Тест: `c.postMessage()` "did
   not throw".

2. **`postMessage(Symbol())` не бросает `DataCloneError`.** Код
   (`broadcast_channel.rs:211-217`):
   ```js
   try {
     json = JSON.stringify(message === undefined ? null : message);
     if (json === undefined) json = 'null';
   } catch (e) {
     throw new DOMException(..., "DataCloneError");
   }
   ```
   `JSON.stringify(Symbol())` не бросает — оно возвращает `undefined`
   (штатное поведение `JSON.stringify` для значений, которые оно не умеет
   сериализовать: `symbol`/`function`/`undefined`). Строка `if (json ===
   undefined) json = 'null'` явно перехватывает этот случай и превращает
   его в `null` вместо `DataCloneError` — символ молча теряется, ошибка
   не долетает никогда.

## Причина

`BroadcastChannel.prototype.postMessage` использует `JSON.stringify` как
суррогат structured clone для передачи payload через процесс-глобальный
`BroadcastHub` (`mpsc::Sender<String>` — канал типизирован на `String`,
отсюда сериализация в JSON как способ пересечь границу потоков). Это
верно решает задачу межпоточной доставки, но семантически не эквивалентно
`structuredClone` (уже существующая, спек-корректная функция в этом же
шиме, `dom.rs:10671`, которую использует `MessagePort.postMessage`):

- `JSON.stringify` не бросает на `Symbol`/`function`/`undefined` — просто
  опускает их (в объекте) или возвращает `undefined` (на верхнем уровне),
  а код выше конвертирует этот `undefined` в `'null'` вместо распознавания
  как "нужно бросить DataCloneError".
- Теряет типы, которые `structuredClone` сохраняет: `Map`/`Set`
  сериализуются как `{}`, `Date` превращается в ISO-строку (а не остаётся
  `Date`-инстансом при получении), `ArrayBuffer`/typed arrays теряют
  бинарную форму, `NaN`/`Infinity`/`-Infinity` становятся `null`,
  свойства со значением `undefined` внутри объекта отбрасываются вместо
  сохранения ключа.
- Циклические структуры бросают `TypeError: Converting circular structure
  to JSON` вместо спек-предписанного `DataCloneError`.

## Дальше

Fix scope (для P3): (1) добавить проверку `arguments.length === 0` →
`TypeError` (тот же паттерн, что уже есть в конструкторе трёмя строками
выше); (2) заменить сериализацию: сначала прогнать `message` через
существующий `structuredClone(message)` (это уже валидирует и бросает
`DataCloneError` на `Symbol`/`function` синхронно, до похода в hub), затем
сериализовать результат клона для передачи по `mpsc::Sender<String>` —
формат передачи через провод может остаться JSON-based (тип канала
менять не обязательно), важно лишь, чтобы валидация клонируемости и
трансляция специальных типов (`Map`/`Set`/`Date`/typed arrays/`NaN`)
проходили через тот же путь, что и `MessagePort`, а не заново через
`JSON.stringify`. Требует восстановить эти типы и на стороне получателя
(`_lumen_deliver_broadcast_messages`) — сейчас там, по всей видимости,
простой `JSON.parse`, симметричный текущей отправке; вне разбора этой
сессии, но стоит проверить при фиксе.
