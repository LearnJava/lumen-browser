# BUG-918 — `unhandledrejection` стреляет по концу МИКРОТАСКА, а не задачи: обработчик, навешенный после первого `await`, уже опоздал

**Статус:** FIXED 2026-08-26
**Заведён:** 2026-08-25 (P1, найден в ходе [BUG-846](BUG-846-FIXED.md) — правка Compression Streams впервые заставила `transform` бросать, и дефект стал наблюдаемым)
**Область:** `crates/js/src/v8_runtime.rs:407` (`scope.enqueue_microtask(flush)` в `lumen_promise_reject_callback`), `:457` (`flush_promise_rejections_callback`)
**Владелец:** P1 (`lumen-js`)

## Симптом

```js
var b = Promise.reject(new TypeError('B'));
(async function(){ await Promise.resolve(); b.catch(function(){}); })();
```

Печатается `[unhandled-rejection] TypeError: B` и на странице стреляет
`unhandledrejection`, хотя обработчик навешен в той же **задаче** — просто
на микротаск позже.

## Прямое измерение

`target/dev-release/lumen.exe --dump-layout <страница>` (2026-08-25, Windows,
dev-release):

| промис | обработчик | ожидалось | получено |
|---|---|---|---|
| A | в том же синхронном ходе | тишина | тишина ✅ |
| B | после одного `await` (та же задача) | тишина | `[unhandled-rejection] TypeError: B-after-await` ❌ |
| C | не навешен вовсе | отчёт | отчёт ✅ |

## Причина

HTML LS §8.1.7.5 «notify about rejected promises» — шаг алгоритма
**«perform a microtask checkpoint»**, то есть отчёт формируется, когда
очередь микротасков уже опустела, по концу задачи. `lumen_promise_reject_callback`
вместо этого откладывает слив на `Isolate::enqueue_microtask`, а микротаск,
поставленный из микротаска, выполняется в том же чекпойнте и **раньше**, чем
до `.then` доберётся код за `await`. Комментарий над функцией это прямо
называет «проще, чем воспроизводить тайминг V8» — граница выбрана на порядок
мельче нужной.

## Масштаб

Наблюдаемое место, где это меняет вердикт целого файла:
`compression/compression-bad-chunks.any.html` — 15 подтестов из 28 стали
PASS после BUG-846, но сам файл ушёл OK → **ERROR**, потому что
`readPromise` там создаётся синхронно, а обработчик получает после
`await promise_rejects_js(t, TypeError, writePromise)`. Соседний
`decompression-bad-chunks.any.html`, где обработчики навешены синхронно,
остался OK. Шаблон «создать промисы, потом по очереди их `await`-ить» —
обычный для `promise_test`, так что цена не ограничена этим файлом.

## Направление починки (не предписание)

Слив должен идти по концу задачи, а не микротаска — та же развилка, что у
[BUG-832](BUG-832-FIXED.md)/[BUG-842](BUG-842-FIXED.md): запись прямо в
`_lumen_timers` с `nesting: 0` вместо `enqueue_microtask`. **Ловушка, из-за
которой это не сделано вместе с BUG-846:** в рантаймах без событийного цикла
(`--dump-layout`/`--dump-display-list`, растеризация SVG, юнит-тесты) таймеры
никто не прокачивает, поэтому наивный перенос сделает отчёт о необработанном
отказе там невидимым — а именно там его диагностическая ценность (BUG-703)
и была доказана. Нужен либо слив по завершению `eval`/пампа на стороне
хоста, либо явный дренаж в конце каждого прогона задач.

## Как проверить фикс

1. Страница-проба выше под `--dump-layout`: строку про `B` печатать нельзя,
   строку про `C` — обязательно.
2. `run_report.py --all --root compression`:
   `compression-bad-chunks.any.html` должен вернуться в OK, не теряя
   подтестов.

---

## Починка (2026-08-26, P1)

Слив перенесён из очереди микротасков в **петлю V8-потока**: `V8Command::Run(job)`
теперь после `job(&mut inner)` зовёт новый `drain_promise_rejections`
(`crates/js/src/v8_runtime.rs`), а `enqueue_microtask(flush)` удалён из обеих
веток `lumen_promise_reject_callback`. `flush_promise_rejections_callback`
перестал быть `v8::FunctionCallback` и стал обычной `fn(&mut v8::PinScope)`.

**Почему именно эта граница, а не `_lumen_timers`.** «Notify about rejected
promises» — шаг 4 алгоритма «perform a microtask checkpoint» (HTML LS
§8.1.7.3), то есть отчёт формируется по опустевшей очереди микротасков.
Прямой хук на это — `Isolate::AddMicrotasksCompletedCallback` (им же
пользуется Node.js), но **в `rusty_v8` 150.1.0 привязки к нему нет**
(`grep MicrotasksCompleted` по `src/` пуст; есть только
`perform_microtask_checkpoint`/`enqueue_microtask`). Зато при авто-политике
микротасков изолят сам сливает очередь до возврата из API-вызова, вошедшего в
JS, — значит к моменту, когда задание отдаёт управление петле, чекпойнт уже
пройден и списки говорят ровно то, что спека просит отчитать. Это и снимает
ловушку из «Направления починки»: через `V8Command::Run` проходит **любой**
вход в JS этого рантайма, включая те, где событийного цикла нет
(`--dump-*`, растеризация SVG, юнит-тесты), поэтому диагностическая строка
BUG-703 остаётся видимой там же, где была.

Цикл в `drain_promise_rejections` (потолок 8 оборотов) нужен потому, что сам
слив зовёт страницу, а та может отклонить свой промис — каждый такой оборот
это новый чекпойнт; потолок лишь не даёт странице, которая отклоняет промис
из собственного `unhandledrejection`, крутить поток вечно (остаток уйдёт после
следующего задания).

## Замер

Проба (`.tmp/rej.html`, `--dump-layout`) — заявка называла один случай, их
оказалось три. До: ложный отчёт на `B` (один `await`), `D` (два `await`) и
`E` (`queueMicrotask`), а следом ещё и ложный `rejectionhandled` на все три.
После: молчат `A`/`B`/`D`/`E`, отчитываются `C` (никем не обработан) и
`F` (обработан в следующей задаче — по спеке отчёт и должен уйти).

A/B по владеющей категории
`html/webappapis/scripting/processing-model-2/unhandled-promise-rejections`
(dev-release, Windows): подтесты **58/128 → 62/128**, harness 6/12 без
изменений, регрессий ноль. Все четыре — ровно про эту границу:
`delayed handling: a microtask delay before attaching a handler prevents both
events` (два варианта) и `microtask nesting: attaching a handler inside a
combination of …` (два варианта).

`run_report.py --all --root compression --recursive`: **19/19** harness OK
(было 18/19), подтесты 242/322 без потерь — `compression-bad-chunks.any.html`
вернулся в OK, как и требовал пункт 2 проверки.

Контроль на промис-тяжёлой категории `streams/readable-streams`: 194/362
подтеста, вердикты подтестов **байт-в-байт** те же; единственная разница —
`tee.any.html` `ERROR | Unhandled rejection` → `TIMEOUT`. Это не регрессия, а
та же чистка с другой стороны: файл и раньше висел на подтесте
`erroring a teed stream should error both branches`, а ярлык ERROR ему давал
именно ложный отчёт; теперь он честно сообщает своё зависание (отдельный,
не этот дефект).

## Остаток

Доставка `unhandledrejection`/`rejectionhandled` синхронна внутри слива, а
HTML LS §8.1.7.5 ставит на неё *задачу*. Наблюдаемой разницы это пока не
даёт (слив и так идёт на границе задач), но порядок относительно других
задач той же итерации цикла спекой не гарантируется.
