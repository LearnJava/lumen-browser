# BUG-846 — Compression Streams отдают результат только после `writer.close()`: чтение до закрытия записывающей стороны не разрешается никогда

**Статус:** FIXED 2026-08-25 (P1)
**Заведён:** 2026-08-22 (WPT-RUN-6, срез 22 — найден живым замером, есть маркер `compression-stream-read-before-close`)
**Область:** `crates/js/src/dom.rs:8215` (`CompressionStream` — `transform` только копит, вся работа в `flush`), `crates/js/src/dom.rs:8232` (`DecompressionStream` — то же), `crates/js/src/dom.rs:8197`–`8198` (комментарий шима, прямо описывающий модель «buffer-then-flush»)
**Владелец:** P1 (`lumen-js`). Заведён P2 в ходе WPT-задачи, починен P1 2026-08-25.

## Симптом

```js
const ds = new DecompressionStream('deflate');
const reader = ds.readable.getReader();
ds.writable.getWriter().write(chunk);   // валидный deflate-чанк
await reader.read();                    // не разрешится никогда
```

Стоит добавить `writer.close()` — и та же цепочка отрабатывает мгновенно и
правильно.

## Прямое измерение

`tests/wpt/verify_perf_idb_sse_gaps.py` (2026-08-22, dev-release, Linux,
коммит `bafa603d9`, `--seconds 6`, страницы живы — 11 тиков; байты
сжатого «expected output» сгенерированы `zlib`, чтобы вход был заведомо
валидным):

| вариант | ожидалось | получено |
|---|---|---|
| `decompression-basic` (чтение без `close`) | `ds-read done=false text=expected output` | `cs-present ds=function cs=function`, `ds-after alive` — и всё |
| `decompression-formats` (gzip + deflate-raw, без `close`) | два `ds-…` | ни одного |
| `decompression-after-close` (контроль) | `ds-closed-read text=expected output` | ровно это |
| `compression-roundtrip` (`write` + `close` + чтение ридером) | `cs-chunk bytes>0` | `cs-chunk done=false bytes=35` |

То есть сам нативный кодек в порядке; ломается модель доставки.

## Причина (локализована чтением кода)

Шим прямо документирует свою модель:

```js
// dom.rs:8197
// Buffer-then-flush model: accumulates all input chunks, compresses atomically at
// flush (TransformStream.writable.close()). Emits a single Uint8Array output chunk.
```

`transform` (`dom.rs:8220`, `:8237`) только складывает чанк в массив, а
`c.enqueue` вызывается исключительно во `flush`. Спека
(<https://compression.spec.whatwg.org/>, §4 «transform algorithm») требует
отдавать всё, что кодек успел выдать, **на каждый чанк**: и deflate, и gzip
инкрементальны.

## Масштаб

Маркер `compression-stream-read-before-close` в `tests/wpt/timeout_audit.py` —
**3 id** остатка снимка WPT-RUN-5: `decompression-correct-input`,
`decompression-extra-input`, `decompression-uint8array-output`. Все три пишут
чанк и читают, не закрывая писателя, поэтому TIMEOUT.

## Направление починки (не предписание)

Отдавать инкрементально: держать нативный декодер между вызовами (или
скармливать накопленный буфер при каждом `transform` и отдавать прирост),
оставив `flush` только для завершения потока. Нативная сторона
(`_decompress_status_prefixed`, `dom.rs:205`) уже возвращает статус-байт,
которым отличается «пока не хватает данных» от «вход повреждён».

## Как проверить фикс

1. `tests/wpt/.venv/bin/python tests/wpt/verify_perf_idb_sse_gaps.py --variant
   decompression-basic --variant decompression-formats` — ожидаются
   `ds-read … text=expected output` и оба `ds-gzip`/`ds-deflate-raw`.
2. WPT: `run_report.py --all --root compression` — семейство
   `decompression-*` должно перестать быть TIMEOUT.

---

## Починено (P1, 2026-08-25)

Кодек переехал из шима в хост и стал **состоянием**: новый модуль
[`crates/js/src/compression.rs`](../crates/js/src/compression.rs) держит живые
кодеки в `thread_local`-реестре по непрозрачной ручке `u32` — та же форма, что
реестр ключей `subtle_crypto`, и по той же причине (изоляция V8 однопоточна, у
каждого воркера свой поток). Наружу четыре натива: `_lumen_cs_new`,
`_lumen_cs_push`, `_lumen_cs_finish`, `_lumen_cs_free`; ответ каждого —
байтовый массив со статус-байтом впереди (`OK` / `ERROR` / `TRAILING_JUNK`).
Одноразовая пара `_lumen_compress_bytes`/`_lumen_decompress_bytes` вместе с
`dom.rs::_decompress_status_prefixed` **удалена**: после правки у неё не
осталось ни одного вызывающего, а «зарегистрировано и никем не зовётся» — ровно
та форма, на которую в этом проекте раз за разом заводят баги (BUG-809,
BUG-839, BUG-852).

### Заявка называла один дефект, их было четыре

Три нашлись прогоном категории, а не чтением названной функции.

1. **Не-BufferSource чанк молча читался как пустой.** `_csToU8` возвращал
   `new Uint8Array(0)` для `undefined`/`null`/числа/объекта/массива, поэтому
   поток спокойно продолжал работу там, где §4 требует провала WebIDL-конверсии,
   обрывающего обе стороны. 48 подтестов (`compression-bad-chunks`,
   `decompression-bad-chunks`), ни одного из которых заявка не касалась.
2. **Усечённое тело декодировалось как УСПЕХ.** Высокоуровневая обёртка
   `flate2::write::ZlibDecoder` не отличает «adler32-хвост не дошёл» от «поток
   кончился штатно» — её `finish()` в обоих случаях `Ok`. Поэтому
   `deflate`/`deflate-raw` переведены на низкоуровневый `flate2::Decompress`,
   где `Status::StreamEnd` виден явно, и «конца потока не было» становится
   ошибкой на `finish`. `gzip` намеренно остался на `write::GzDecoder`: его
   `finish()` сам проверяет CRC32 и ISIZE (и что все 8 байт хвоста дошли), а
   переписывать разбор gzip-заголовка (FEXTRA/FNAME/FCOMMENT/FHCRC) значило бы
   рисковать ровно теми случаями, которые `decompression-corrupt-input` и
   проверяет.
3. **Мусор за концом потока** отличается от повреждённого входа и обязан
   отдать уже раскодированные байты **перед** обрывом. Признак — тот самый
   `Ok(0)`, который flate2 документирует для записи после конца члена. Порядок
   здесь не косметика: `ReadableStreamDefaultControllerError` сбрасывает
   очередь, поэтому «оборвать, потом отдать» теряет значение, а
   `decompression-extra-input` требует, чтобы первый `read()` его получил и
   отказал только следующий.

Четвёртый нашёлся тем, что тест упал не по своей причине: **`zio::Writer`
сливает свой внутренний буфер в приёмник в НАЧАЛЕ следующего вызова, а не в
конце текущего**, поэтому целиком поданный gzip-поток отдавал ноль байт до
`finish` — то есть ровно тот симптом, который правка чинила, только на одном
формате из трёх. Лечится пустым `write(&[])` (`pump`) перед сбором выхода.

### Замер A/B по всей категории

Windows, dev-release, `run_report.py --all --root compression --recursive`,
19 файлов / 322 подтеста.

| файл | до | после |
|---|---|---|
| `decompression-correct-input` | TIMEOUT 0/4 | **OK 3/4** |
| `decompression-extra-input` | TIMEOUT 0/4 | **OK 3/4** |
| `decompression-uint8array-output` | TIMEOUT 0/4 | **OK 3/4** |
| `decompression-bad-chunks` | OK 0/36 | **OK 27/36** |
| `compression-bad-chunks` | OK 0/28 | ERROR 15/28 |
| `decompression-corrupt-input` | OK 22/29 | **OK 26/29** |
| остальные 13 | без изменений | без изменений |
| **итого** | 187/322, harness OK 16/19 | **242/322, harness OK 18/19** |

Остаток целиком объясним и вне этого бага: **brotli** — формата в движке нет,
а `resources/formats.js` перечисляет его наравне с тремя рабочими, то есть
четверть подтестов всей категории отваливается на конструкторе; и
`SharedArrayBuffer`-чанки, которые движок не отличает от обычного буфера.

### Цена: один файл ушёл OK → ERROR не своим дефектом

`compression-bad-chunks` набрал 15 новых PASS и при этом упал в ERROR:
`readPromise` там создаётся синхронно, а обработчик получает после
`await promise_rejects_js(t, TypeError, writePromise)`, и отчёт о
необработанном отказе промиса в этом движке формируется по концу **микротаска**,
а не задачи. Соседний `decompression-bad-chunks`, где обработчики навешены
синхронно, остался OK. Измерено отдельной пробой под `--dump-layout` и
заведено как [BUG-918](BUG-918-OPEN.md); здесь не чинится, потому что живёт в
`v8_runtime.rs`, меняет поведение всего корпуса разом и упирается в рантаймы
без событийного цикла.

### Наблюдаемое изменение, которое нужно знать пробе

Компрессор тоже отдаёт выход по ходу, а не одним куском на `close()`, поэтому
первый чанк gzip — это его заголовок. Читатель обязан **собирать все** чанки
(как это делает WPT-шный `concatenate-stream.js`), а не первый: четыре
собственных юнит-теста в `dom.rs` пришлось переписать именно на сбор, и они бы
молча прошли, читая один чанк, для двух форматов из трёх.

### Остаток

Поток, брошенный без `close()` и без ошибки, оставляет кодек в реестре до конца
жизни рантайма: у шимового `TransformStream` нет хука `cancel`, а `abort`
писателя и `cancel` читателя до трансформера не доходят. Граница — один
документ (реестр `thread_local`, рантайм умирает вместе со страницей).
