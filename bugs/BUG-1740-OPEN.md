# BUG-1740 — классы Streams не имеют WebIDL-формы и не проверяют аргументы конструкторов

**Статус:** OPEN
**Заведён:** 2026-10-27 (P1, WPT-STREAMS-SCOUT)
**Область:** js (`crates/js/src/shim/streams_shim.js` — `ReadableStream`, `WritableStream`, `TransformStream`, контроллеры, читатели/писатель, `CountQueuingStrategy`/`ByteLengthQueuingStrategy`)

## Симптом

Проба (`--dump-layout` на локальной странице):

```
ReadableStream.length / WritableStream.length / TransformStream.length            → 2 / 2 / 3   (WebIDL: 0 / 0 / 0)
Object.keys(ReadableStream.prototype)  → getReader|cancel|_rs_do_cancel|tee|values|pipeTo|pipeThrough
Object.prototype.toString.call(new ReadableStream())                               → "[object Object]"
new ReadableStream(null)                                                           → не бросает (TypeError)
new ReadableStream({start: 'x'}) / {pull: {}} / {cancel: '2'}                      → не бросают
new WritableStream({type: 'bytes'}), new WritableStream({start: 'test'})           → не бросают
new CountQueuingStrategy()                                                         → не бросает (словарь init обязателен)
new ReadableStreamDefaultReader('potato')                                          → не бросает
```

- Методы WebIDL-интерфейса должны быть неперечисляемыми; `_rs_do_cancel` — служебная функция шима на публичном прототипе.
- Нет `Symbol.toStringTag` у классов и контроллеров.
- Ошибка геттера `size`/`highWaterMark` стратегии не пробрасывается из конструктора; `NaN`/отрицательный `highWaterMark`, не-функция `size` не отвергаются (`readable-streams/bad-strategies` 0/24, `writable-streams/bad-strategies` 12/21).
- `desiredSize` копит ошибку округления — спецификация сбрасывает накопленную сумму в 0, когда очередь пуста (`floating-point-total-queue-size` 0/12).
- `reader.closed` не меняет идентичность после `releaseLock()` (6 сабтестов `templated`, `default-reader` 2).

## Как найдено

WPT-STREAMS-SCOUT, кластер C (`docs/wpt-vendor-notes/streams.md`): `idlharness.any.js` 123/228 (15× `property has wrong .length`, 16× `.name`, 13× `should not be enumerable`), `queuing-strategies.any.js` 9/20, `readable-streams/{bad-strategies,bad-underlying-sources,constructor,default-reader,floating-point-total-queue-size,templated}`, `writable-streams/{bad-strategies,bad-underlying-sinks,constructor}`. Потолок ≈ 24 балла (0.035 п.п. корпуса), 37 + 45 id во всех вариантах окружения.

## Что делать

1. Определить интерфейсы так, чтобы `Object.getOwnPropertyDescriptors` совпадали с IDL: `length`, неперечисляемые методы, `Symbol.toStringTag`, служебные функции — вне публичных прототипов (замыкание или `Symbol`).
2. Привести разбор аргументов конструкторов к `UnderlyingSource`/`UnderlyingSink`/`QueuingStrategy` из WebIDL: словарь, не-функция `start`/`pull`/`cancel`/`write`/`close`/`abort` → `TypeError`, порядок чтения свойств (`readable-streams/constructor.any.js` проверяет его), `type` вне `"bytes"`/`undefined` для `ReadableStream`, `type` задан у `WritableStream` → `RangeError`.
3. `ExtractHighWaterMark`/`ExtractSizeAlgorithm` по спецификации (§7.1); `desiredSize` — пересчёт суммы очереди с обнулением при пустой очереди.

## Как проверить

Первые четыре строки пробы дают `0 / 0 / 0`, пустой список перечисляемых ключей, `[object ReadableStream]`, `TypeError`; `run_corpus.py --prefixes streams/idlharness,streams/queuing-strategies,streams/readable-streams/bad-strategies,streams/writable-streams` без регрессий; `idlharness.any.html` ≥ 200/228.
