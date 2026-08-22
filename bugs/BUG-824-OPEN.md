# BUG-824 — Streams: `tee()` закрывает исходный поток и теряет чанки, BYOB-ридер подменяется обычным, async-итерации нет, `TextDecoderStream` не закрывает свою читаемую сторону

**Статус:** OPEN
**Заведён:** 2026-08-21 (WPT-RUN-6, срез 19 — общий с [BUG-823](BUG-823-OPEN.md) маркер `streams-promise-unsettled`, 40 id)
**Область:** `crates/js/src/dom.rs:7391-7405` (`ReadableStream.tee`), `:7374` (`getReader` — аргумент не читается), `:7353` (конструктор игнорирует `type: 'bytes'`), `:7603` (`TransformStream`), плюс `TextDecoderStream` в том же шиме
**Владелец:** P1/P3 (`lumen-js`). Заведён P2 в ходе WPT-задачи, здесь не чинится.

## Симптом

Четыре независимых поверхности Streams отсутствуют или сделаны так, что
тест на них зависает, а не падает:

```js
// 1. tee() закрывает источник и раздаёт только то, что уже лежало в очереди
const rs = new ReadableStream({ start(c) { setTimeout(() => { c.enqueue("late"); c.close(); }, 50); } });
const [a, b] = rs.tee();
rs.locked            // false — по спеке true
await a.getReader().read();   // ← не оседает: "late" пришёл уже в закрытый поток

// 2. BYOB молча деградирует
const reader = bytes.getReader({ mode: "byob" });
reader.constructor.name       // "ReadableStreamDefaultReader"
typeof ReadableStreamBYOBReader  // "undefined"

// 3. асинхронной итерации нет
for await (const chunk of rs) {}   // TypeError: rs is not async iterable

// 4. TextDecoderStream не закрывает читаемую сторону
writer.write(bytes); writer.close();
await reader.read();   // первый чанк приходит, второй (done) — никогда
```

## Прямое измерение

`tests/wpt/verify_stream_scroll_message_gaps.py` (2026-08-21, коммит
`6e60c8aa8`, `--seconds 6`, все страницы живы — по 11 тиков):

| проба | получено |
|---|---|
| `stream-tee` | `tee-locked=false` — и ни `tee0`, ни `tee1` |
| `stream-byob` | `byob-ctor-ok`, `byob-reader=ReadableStreamDefaultReader`, `has-byob-request=undefined`, `byob-read n=3` (переданный `Uint8Array(3)` не использован) |
| `stream-async-iter` | `asynciter=undefined`, `iter-threw TypeError: rs is not async iterable` |
| `stream-textdecoder` | `decode0 v=€` — и `decode1 done` не приходит никогда |
| `stream-transform-close` | **контроль**: рукописный `TransformStream` закрытие проводит (`tclose-read1 done`), значит дело именно в `TextDecoderStream` |

Пофайловый прогон (см. [BUG-823](BUG-823-OPEN.md), та же обвязка) сажает на
эти поверхности `readable-streams/tee.any.js` (первый зависший —
«canceling both branches should aggregate the cancel reasons»),
все четыре `readable-byte-streams/*` и пять `encoding/streams/*`
(`decode-incomplete-input.any.js` — оба его сабтеста стартуют и ни один не
завершается).

## Причина (локализована чтением кода)

* **`tee()` (`dom.rs:7391`)** копирует *текущую* очередь контроллера,
  делает `_rs_do_close(self)` на источнике и отдаёт две независимые
  копии-заглушки. То есть: источник закрывается (по спеке он должен
  остаться читаемым и **заблокированным**), `locked` остаётся `false`, а
  всё, что источник заэнкьюит после вызова, не попадает никуда.
* **BYOB.** Конструктор (`:7353`) не смотрит на `type`, `getReader`
  (`:7374`) не читает свой аргумент, классов
  `ReadableStreamBYOBReader`/`ReadableStreamBYOBRequest` нет. Тест получает
  обычный ридер и обычный чанк — то есть тихую подмену семантики вместо
  ошибки.
* **Async-итерация.** `Symbol.asyncIterator` на прототипе не определён.
* **`TextDecoderStream`.** Закрытие записываемой стороны не доводится до
  читаемой (нет шага flush → close), поэтому `readableStreamToArray` —
  штатный хелпер всей категории `encoding/streams` — не дожидается `done`.

## Масштаб

Отдельного числа у этого бага нет: маркер `streams-promise-unsettled` общий
с [BUG-823](BUG-823-OPEN.md) (40 id), потому что по исходнику теста эти две
причины неразделимы — файл `readable-byte-streams/general.any.js` одинаково
подходит под обе. Внутри этих 40 на долю поверхностей отсюда приходятся
4 id `readable-byte-streams`, 5 `encoding/streams` и `readable-streams/tee`.

Вне WPT: `tee()` — штатный способ прочитать тело ответа дважды
(кэш + разбор), и на Lumen вторая копия молча приходит пустой; BYOB —
основа эффективного чтения бинарных потоков; `for await` по потоку —
самая частая форма их чтения в современном коде.

## Направление починки (не предписание)

По убыванию отдачи и по возрастанию цены: (1) `Symbol.asyncIterator` —
тонкая обёртка над `getReader()`, десяток строк; (2) закрытие
`TextDecoderStream` (провести close/flush через трансформ, механизм уже
работает у рукописного `TransformStream` — см. контроль выше);
(3) настоящий `tee()` — две ветки с общим ридером источника, источник
остаётся заблокированным; (4) BYOB — самая дорогая часть, требует
`ReadableByteStreamController` с `byobRequest`, и её разумно отложить
отдельно от (1)–(3).

## Как проверить фикс

1. `tests/wpt/.venv/bin/python tests/wpt/verify_stream_scroll_message_gaps.py
   --variant stream-tee --variant stream-byob --variant stream-async-iter
   --variant stream-textdecoder` — все четыре печатают ожидаемые маркеры.
2. WPT: `run_report.py --all --root encoding/streams --recursive` и
   `--root streams/readable-byte-streams --recursive` перестают висеть.
