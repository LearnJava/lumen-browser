# BUG-1741 — `TransformStream`: нет `transformer.cancel`, не проверяются `readableType`/`writableType`, `terminate()` не закрывает запись

**Статус:** OPEN
**Заведён:** 2026-10-27 (P1, WPT-STREAMS-SCOUT)
**Область:** js (`crates/js/src/shim/streams_shim.js`, `TransformStream` и `TransformStreamDefaultController`)

## Симптом

```
var called = 0; var ts = new TransformStream({ cancel(r) { called++; } });
ts.readable.cancel('x');                                  → called === 0   (спецификация: transformer.cancel(reason) вызван)
new TransformStream({ readableType: 'bytes' })            → не бросает     (спецификация: RangeError; то же для writableType)
```

Кроме проб:

- после `controller.terminate()` запись в `writable` не отвергается `TypeError`-ом, а `transform()` зовётся ещё раз (`terminate.any.js` 2/6: «abort() should reject with a TypeError», «transform() should have seen no chunks»);
- исключение из `start()` или из `size()` стратегии не пробрасывается из конструктора (`errors.any.js` 14/21);
- `controller.enqueue()` без аргумента не бросает (`general.any.js`: «enqueue should throw»);
- отмена readable-стороны не отвергает `closed` писателя с `TypeError` (`general.any.js`).

## Как найдено

WPT-STREAMS-SCOUT, кластер TS (`docs/wpt-vendor-notes/streams.md`): `transform-streams/{cancel 1/11,terminate 2/6,errors 14/21,flush 5/6,general 21/26}`, 24 id, 213/294 сабтеста, потолок ≈ 6.8 балла (0.010 п.п. корпуса).

## Что делать

Привести `TransformStream` к §6 (TransformStreamDefaultSinkAbortAlgorithm / SinkCloseAlgorithm / SourceCancelAlgorithm): вызывать `transformer.cancel` на отмену readable-стороны и на `abort()` writable-стороны с взаимной блокировкой «кто успел первым»; `terminate()` = закрыть readable + `error` writable с `TypeError`; отвергать `readableType`/`writableType` до чтения остальных полей; пробрасывать исключения `start`/`size`. Взаимодействие `transform()` и back-pressure без читателя (`backpressure.any.js`) — отдельный корень (синхронный `pull`), задача STREAMS-LAZY-PULL.

## Как проверить

Обе строки пробы: `called === 1` и `RangeError`; `run_corpus.py --prefixes streams/transform-streams` — `cancel.any.html` ≥ 9/11, `terminate.any.html` 6/6, `errors.any.html` 21/21.
