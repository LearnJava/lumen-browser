# BUG-824 — Streams: `tee()` закрывает исходный поток и теряет чанки, BYOB-ридер подменяется обычным, async-итерации нет, `TextDecoderStream` не закрывает свою читаемую сторону

**Статус:** FIXED 2026-08-25
**Заведён:** 2026-08-21 (WPT-RUN-6, срез 19 — общий с [BUG-823](BUG-823-FIXED.md) маркер `streams-promise-unsettled`, 40 id)
**Область:** `crates/js/src/dom.rs:7391-7405` (`ReadableStream.tee`), `:7374` (`getReader` — аргумент не читается), `:7353` (конструктор игнорирует `type: 'bytes'`), `:7603` (`TransformStream`), плюс `TextDecoderStream` в том же шиме
**Владелец:** P1 (`lumen-js`). Заведён P2 в ходе WPT-задачи, починен P1 2026-08-25.

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

Пофайловый прогон (см. [BUG-823](BUG-823-FIXED.md), та же обвязка) сажает на
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
с [BUG-823](BUG-823-FIXED.md) (40 id), потому что по исходнику теста эти две
причины неразделимы — файл `readable-byte-streams/general.any.js` одинаково
подходит под обе. Внутри этих 40 на долю поверхностей отсюда приходятся
4 id `readable-byte-streams`, 5 `encoding/streams` и `readable-streams/tee`.

Вне WPT: `tee()` — штатный способ прочитать тело ответа дважды
(кэш + разбор), и на Lumen вторая копия молча приходит пустой; BYOB —
основа эффективного чтения бинарных потоков; `for await` по потоку —
самая частая форма их чтения в современном коде.

## Перезамер 2026-08-22 (срез 22): `Response` над потоком теряет тело

`tests/wpt/verify_perf_idb_sse_gaps.py --variant response-from-stream`
(dev-release, Linux, коммит `bafa603d9`): `new Response(rs).arrayBuffer()`
разрешается **нулём байт** — и для потока из `CompressionStream` (в котором
через собственный ридер читается чанк в 35 байт, `--variant
compression-roundtrip`), и для рукотворного `ReadableStream`, отдающего три
байта и закрывающегося. То есть ещё одна отсутствующая поверхность интеграции
Streams ↔ Fetch: тело-поток до `Response` не доезжает. Отдельного бага не
заводится — это тот же класс, что перечисленные выше `tee()`/BYOB/
`Symbol.asyncIterator`.

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

## Починка (2026-08-25, P1, ветка `p1-bug824-streams`)

Правка целиком в JS-шиме (`crates/js/src/dom.rs`, `WEB_API_SHIM`).

**Перезамер до правки** (юнит-проба в `v8_whatwg_streams`, после влитого
BUG-823): `tee-locked=false | byob-ctor=ok | has-byob-reader=undefined |
asynciter=undefined | tee0=early | tee1=early | tds0=hi done=false |
resp-bytes=0 | tds1 done=true`. То есть **закрытие `TextDecoderStream`
починилось само** — побочный эффект переписанной записываемой стороны в
BUG-823 (сток `close` теперь доходит до `_ts_flush`, а тот закрывает
читаемую сторону). Остальные четыре поверхности были на месте, включая
нулевое тело `Response` из перезамера среза 22.

1. **`tee()` — §3.2.6 целиком.** Источник берётся под общий ридер и остаётся
   `readable` и `locked === true`; обе ветки тянут через один
   `pullAlgorithm` с флагами `reading`/`readAgain`, так что конкурирующего
   чтения не возникает. Отмена ветки не трогает источник: источник
   отменяется только когда отменены **обе**, и с агрегированной причиной
   `[reason1, reason2]` — то самое «canceling both branches should aggregate
   the cancel reasons», на котором файл `readable-streams/tee.any.js` вставал
   первым.
2. **`Symbol.asyncIterator` + `ReadableStream.prototype.values`.** Обёртка
   над `getReader()`; `return()` (выход из `for await` по `break`) отменяет
   поток и снимает блокировку, иначе поток остался бы залоченным навсегда.
3. **BYOB — §3.8/§3.10/§3.11.** Добавлены `ReadableByteStreamController`,
   `ReadableStreamBYOBReader`, `ReadableStreamBYOBRequest`; конструктор
   читает `type`/`autoAllocateChunkSize` и бросает `TypeError` на чужой
   `type`, `getReader({mode:'byob'})` — на не-байтовый поток. Ридеры делят
   `closed`/`cancel`/`releaseLock` через `_rs_install_reader_common`.
   **Осознанное отступление:** спека *передаёт* (детачит) буфер вызывающего
   и возвращает вид на перенесённую копию; `ArrayBuffer.prototype.transfer`
   в движке не заведён, поэтому буфер переиспользуется — страница, сохранившая
   ссылку на исходный вид, у нас всё ещё видит байты.
4. **Тело-поток у `Response`/`Request`.** `extractBody` больше не подменяет
   поток пустым телом: поток становится телом как есть (`resp.body === rs`),
   `consume()` для такого тела вычитывает его через новый
   `_rs_drain_to_bytes`, а `clone()` делает `tee()` — то есть каноничный
   сценарий, ради которого `tee()` и нужен (прочитать тело дважды), теперь
   работает целиком.

**Замер после правки** (та же проба): `tee-locked=true |
has-byob-reader=function | byob-reader=ReadableStreamBYOBReader |
byob0=7,8 done=false | byob1=9 done=false | asynciter=function | aiter=a,b |
resp-bytes=3`.

**Тесты:** 10 новых в `dom.rs::tests::v8_whatwg_streams` (поздние чанки через
`tee`, агрегация причин отмены, BYOB-чтение и `byobRequest`, отказ `byob` на
не-байтовом потоке, async-итерация и её `break`, тело-поток у `Response` и
его `clone()`, плюс сторож на закрытие `TextDecoderStream`). `cargo test -p
lumen-js --features v8-backend` — 3116 пройдено, 0 упало; clippy чист.

**Остаётся вне рамок:** `DecompressionStream` по-прежнему отдаёт всё разом
только на `close()` ([BUG-846](BUG-846-OPEN.md)); передача (detach) буфера
при BYOB-чтении — см. отступление выше.
