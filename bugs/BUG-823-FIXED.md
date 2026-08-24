# BUG-823 — Streams: промисы разрешаются только на счастливом пути, любая ошибка/закрытие/отмена оставляет промис висеть навсегда

**Статус:** FIXED 2026-08-25 (P1, ветка `p1-bug823-streams`)
**Заведён:** 2026-08-21 (WPT-RUN-6, срез 19 — 40 TIMEOUT остатка вместе с [BUG-824](BUG-824-OPEN.md), механизм `streams-promise-unsettled`)
**Область:** `crates/js/src/dom.rs:7303-7620` — весь шим `ReadableStream`/`WritableStream`/`TransformStream` (в частности `ReadableStreamDefaultController.error` `:7331`, `_rs_do_close` `:7342`, `ReadableStream.pipeTo` `:7406`, конструктор `ReadableStream` `:7353` — `start()` вызывается синхронно и его возвращаемое значение выбрасывается)
**Владелец:** P1/P3 (`lumen-js`). Заведён P2 в ходе WPT-задачи, здесь не чинится.

## Симптом

Поток на счастливом пути работает; как только тест уводит его в ошибку,
закрытие или отмену — соответствующий промис не резолвится и не
реджектится, и `promise_test` висит до таймаута враннера. Так как
`testharness.js` гоняет `promise_test`-ы последовательно, один такой
подтест уносит с собой **весь остаток файла** — отсюда TIMEOUT целого
`.any.html`, а не FAIL одного сабтеста.

```js
// streams/writable-streams/close.any.js, 4-й promise_test — первый, который
// движок не заканчивает
const ws = new WritableStream({ close() { throw error1; } });
const writer = ws.getWriter();
return Promise.all([
  writer.write('y'),
  promise_rejects_exactly(t, error1, writer.close()),   // ← реджектится верно
  promise_rejects_exactly(t, error1, writer.closed)     // ← не оседает никогда
]);
```

## Прямое измерение

**(1) Точечные пробы** — `tests/wpt/verify_stream_scroll_message_gaps.py`
(2026-08-21, коммит `6e60c8aa8`, `--seconds 6`, все страницы живы):

| проба | ожидалось | получено |
|---|---|---|
| `stream-read-queued` | `read0 v=a, read1 v=b, read2 done` | всё, как ожидалось — **контроль** |
| `stream-pull-demand` | `pull`/`read` вперемешку 0..3 | всё, как ожидалось — **контроль** (модель pull работает, вопреки комментарию `dom.rs:7305`) |
| `stream-async-start` | `read0 v=late, closed` | всё, как ожидалось — **контроль** |
| `stream-close-throws` | `close-rejected`, `closed-rejected` | только `close-rejected boom` — `writer.closed` не оседает |
| `stream-write-throws` | `write-rejected`, `closed-rejected` | только `write-rejected boom` |
| `stream-abort` | `sink-abort why`, `abort-resolved`, `closed-rejected` | только `abort-resolved` — колбэк `abort()` синка не вызван вовсе |
| `stream-transform-close` | `tclose-read1 done` | всё, как ожидалось — **контроль**: закрытие через `TransformStream` работает |

**(2) Пофайловый прогон всех 38 streams-id остатка** (throwaway-обвязка:
реальный тест над локальным http, `add_result_callback` логирует каждый
завершившийся сабтест, 9 с на файл). Из 36 запускаемых (`transferable/*` —
не `.any.js`) **35 останавливаются посреди файла**, и первый незакрытый
сабтест каждый раз именно такой формы:

| файл | сабтестов запущено / закрыто | первый зависший |
|---|---|---|
| `writable-streams/close.any.js` | 26 / 3 | «when the sink throws during close … stream should become errored» |
| `writable-streams/error.any.js` | 5 / 1 | «controller.error() should error the stream» |
| `writable-streams/start.any.js` | 8 / 4 | «returning a thenable from start() should work» |
| `writable-streams/bad-underlying-sinks.any.js` | 14 / 10 | «write: throwing method should cause write() and closed to reject» |
| `readable-streams/general.any.js` | 38 / 17 | «start should be able to return a promise and reject it» |
| `readable-streams/bad-underlying-sources.any.js` | 22 / 6 | «pull: throwing method (second pull)» |
| `transform-streams/errors.any.js` | 21 / 4 | «errors thrown in transform put the writable and readable in an errored state» |
| `transform-streams/backpressure.any.js` | 14 / 7 | «writer.closed should resolve after readable is canceled during start» |

## Причина (локализована чтением кода)

Шим (`dom.rs:7303`) прямо описан в своей шапке как синхронная модель под
`fetch`: «all chunks are enqueued at construction time». Отсюда четыре
конкретных пробела, каждый из которых оставляет промис вечно pending:

* **`writer.closed` не связан ни с чем.** Промис создаётся, но ни ошибка
  контроллера, ни бросок синка, ни `abort()` до него не доходят.
* **`controller.error(e)` (`:7331`) метит поток `errored` и разрешает только
  уже стоящие `read`-запросы.** Промисы `closed`/`ready`/`pipeTo`, взятые
  до или после, не получают ничего.
* **Возвращаемое значение `start()` игнорируется** (`:7360`): промис или
  thenable не ожидается, поэтому «start вернул reject» не приводит поток в
  `errored`, а тест ждёт этого перехода.
* **`abort()` синка не вызывается** — в шиме есть только `cancel` для
  читаемой стороны; `WritableStream.abort` резолвит свой промис и на этом
  всё.

Общая черта: состояние потока меняется, но у шима нет реестра ожидающих
промисов, которым это состояние надо разослать.

## Масштаб

Механизм `streams-promise-unsettled` забирает **40 id** остатка снимка
WPT-RUN-5 (вместе с [BUG-824](BUG-824-OPEN.md), маркер у них общий — по
источнику эти две причины неразделимы): все 33 непонятых TIMEOUT `streams/*`,
5 `encoding/streams` и 1 `fetch/api`, плюс один id переехал сюда из слабой
стадии «что-то бросило».

Оценка снизу: `streams/*` в снимке большей частью не дошёл до своих тестов
по более старшим причинам (worker-варианты — [BUG-778](BUG-778-FIXED.md),
`.https.*` — [BUG-792](BUG-792-OPEN.md)). Вне WPT цена прямая: `fetch()` с
потоковым телом, `pipeTo` в файл, любой конвейер с обработкой ошибок на
Lumen молча зависает вместо того чтобы отдать ошибку.

## Направление починки (не предписание)

Прописать в шиме то, что спека называет состоянием потока: хранить
`[[storedError]]` и списки ожидающих промисов (`closed`, `ready`,
`writeRequests`, `pipeTo`), и при переходе в `errored`/`closed` разослать
их все. Тогда три из четырёх пробелов закрываются одной правкой; отдельно
остаётся ожидание промиса из `start()`/`write()`/`close()` синка — то есть
сделать вызовы алгоритмов асинхронными, как в спеке. Порядок по отдаче:
`writer.closed` (самый частый первый зависший), затем `controller.error`,
затем `start()`-промис, затем `abort()`.

## Как проверить фикс

1. `tests/wpt/.venv/bin/python tests/wpt/verify_stream_scroll_message_gaps.py
   --variant stream-close-throws --variant stream-write-throws
   --variant stream-abort` — каждая проба печатает все свои маркеры.
2. WPT: `run_report.py --all --root streams --recursive` — файлы
   `writable-streams/*` и `transform-streams/*` доходят до конца (сколько
   при этом PASS — отдельный вопрос, здесь важен сам факт завершения).

## Фикс (2026-08-25, P1)

`WritableStream` переписан по машине состояний спеки (§4), а на читаемой
стороне закрыты три точечных пробела. Ключ — **реестр промисов**: у потока
теперь есть `_ws_writeRequests`, `_ws_inFlightWrite`, `_ws_closeRequest`,
`_ws_inFlightClose`, `_ws_pendingAbort`, а у писателя — дефериды
`_readyD`/`_closedD` со своим состоянием, так что переход в
`errored`/`closed` рассылает **все** ожидающие промисы (`_ws_finish_erroring`
→ write-запросы → close-запрос → `writer.closed` → abort-запрос), а не тот
один, на котором пришла ошибка.

Что это поменяло по четырём пробелам заявки:

* **`writer.closed`** — прототипный геттер над `_closedD`; оседает и на
  успешном закрытии (`_ws_finish_in_flight_close`), и на любой ошибке
  (`_ws_reject_close_and_closed_if_needed`), и на `releaseLock()`.
* **`controller.error(e)`** — идёт через `WritableStreamStartErroring`
  (сначала `writer.ready`, потом, когда нет операции «в полёте», полный
  `FinishErroring`). На читаемой стороне парный `_rs_do_error` рассылает
  ошибку и стоящим `read`-запросам, и `reader.closed`.
* **Промис из `start()`** — ожидается на обеих сторонах: у writable
  контроллер не начинает разбирать очередь, пока `start()` не осел
  (`_ws_ctrl_setup`), у readable поток остаётся «не стартовавшим» и
  реджект переводит его в `errored`.
* **`abort()` синка** — вызывается из `_ws_finish_erroring` (спековые
  `AbortSteps`), с `AbortController`/`controller.signal` в придачу.

Заодно: `TransformStream` связан в обе стороны (бросок в `transform`/`flush`
роняет обе половины, `readable.cancel()` роняет writable), `pipeTo` снимает
блокировки и реджектится вместо зависания, `ReadableStream.cancel()` отдаёт
результат `cancel()` источника, а `pull()` больше не зовётся реентрантно и
его реджект ошибает поток. Спековая пометка `[[PromiseIsHandled]]`
воспроизведена `_stream_mark_handled`, иначе отклонённый движком
`writer.closed` вылезал бы страничным `unhandledrejection` (BUG-716).

**Сдвиг контракта, который стоит знать:** синк больше не вызывается в том же
такте, что `write()`/`close()` — очередь двигается только после того, как
осел промис вокруг `start()` (§4.8.3). Два юнит-теста, писавшие проверку в
том же `eval()`, разнесены на два — это спековое поведение, а не регрессия.

**Замеры.** Живая проба (dev-release, Windows, `--seconds 6`): все семь
вариантов печатают ожидаемые маркеры, включая три бывших молчащих —
`stream-close-throws` → `write-resolved, close-rejected boom, closed-rejected
boom`; `stream-write-throws` → `write-rejected boom, closed-rejected boom`;
`stream-abort` → `sink-abort why, abort-resolved, closed-rejected why`; четыре
контроля (`stream-read-queued`, `stream-pull-demand`, `stream-async-start`,
`stream-transform-close`) не сдвинулись. Юнит-тесты: 10 новых в
`dom.rs::tests::v8_whatwg_streams` (по одному на каждый пробел заявки плюс
`pipeTo`, отмена, две стороны `TransformStream`), весь `lumen-js` — 3106
пройдено, 0 упало.

**Вне рамок** (остаётся [BUG-824](BUG-824-OPEN.md)): `tee()` по-прежнему
клонирует снимок очереди вместо ветвления, нет BYOB, `Symbol.asyncIterator`
и закрытия `TextDecoderStream`; поэлементная отдача `DecompressionStream` —
[BUG-846](BUG-846-OPEN.md).
