# BUG-1204 — контроллеры и `ReadableStreamBYOBRequest` конструируются страницей через `new`, хотя конструктора у них нет

**Статус:** OPEN
**Компонент:** js (`crates/js/src/shim/streams_shim.js`)
**Найден:** P3, при фиксе BUG-684, 2026-09-28

## Симптом

WHATWG Streams не определяет конструктор для
`ReadableStreamDefaultController`, `ReadableByteStreamController`,
`ReadableStreamBYOBRequest`, `WritableStreamDefaultController` и
`TransformStreamDefaultController` — их создаёт только сам стрим, а
`new X()` со страницы по WebIDL обязан бросать `TypeError: Illegal
constructor`. В шиме это обычные ES5-функции: `new
ReadableStreamDefaultController({})` молча строит объект, который
проходит `instanceof`, а его методы (`enqueue`, `close`, `error`)
работают над подсунутым страницей «стримом».

После BUG-684 вызов **без** `new` уже бросает (`_stream_require_new`), так
что утечки полей в `globalThis` нет; остаётся только конструирование.

## Причина

Внутренние места создания (`new ReadableStreamDefaultController(stream)` в
конструкторе `ReadableStream` и т.п.) и публичный интерфейсный объект —
одна и та же функция, поэтому безусловный `throw` в ней сломает сам стрим.

## Дальше

Тот же приём, что в BUG-681/BUG-672: интерфейсный объект бросает всегда, а
шим строит экземпляры через `Object.create(X.prototype)` (или ключ-капчу,
если где-то нужна цепочка конструкторов). Проверка — WPT
`streams/idlharness.any.js` (раздел «interface object … must throw when
called as a constructor»).
