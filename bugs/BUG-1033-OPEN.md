# BUG-1033 — poll-based media-error detection делает `unhandledrejection` для `<audio>`/`<video>` недетерминированным

**Статус:** OPEN (частичный митигейт 2026-09-07 — гонка СНИЖЕНА, не устранена)
**Найден:** P3 2026-09-07, побочно при локализации [BUG-1022](BUG-1022-OPEN.md) (`html/semantics` — три
`--check` подряд дают три разных набора регрессий)
**Компонент:** js (`crates/js/src/audio_element.rs:236-301,495-531` — poll-based `play()`/`load()`
через `setInterval(fn, 50)`, флаг ошибки читается из фонового потока декодера; тот же паттерн
подозревается в `video_bindings.rs`, не проверено этим срезом)
**Владелец:** P1/P3

## Симптом

`autoplay.html` (`html/semantics/embedded-content/media-elements/ready-states/`) через
`run_smoke.py` (изолированный прогон, без полной категории) — 10 повторов подряд дают ДВА разных
harness-статуса при АБСОЛЮТНО идентичном наборе 10/10 FAIL-сабтестов:

```
tests/wpt/.venv/bin/python tests/wpt/run_smoke.py --binary target/dev-release/lumen \
  /html/semantics/embedded-content/media-elements/ready-states/autoplay.html
# 9 из 10 повторов: Test ERROR, expected OK. Subtests passed 0/10. Unexpected 10
# 1 из 10 повторов:  Test OK.                 Subtests passed 0/10. Unexpected 10
```

Это настоящий флак, воспроизводимый БЕЗ контекста полной категории (в отличие от
`table-rows.html`/`table-insertRow.html`/`tHead.html` — см. §Отличие от BUG-1022 ниже).

## Механизм (полностью внутри процесса Lumen, не BiDi-транспорт)

`crates/bidi-server` не реализует ни `log.entryAdded`, ни `javascriptError` — никакого
BiDi log-события для необработанных отказов нет вовсе (`grep` по `crates/bidi-server/src/*.rs`).
Гонка целиком на JS-стороне документа теста:

1. `autoplay.html` вызывает `e.play()` без `.catch()` (валидно по спеке) на 10 медиа-элементах
   (5×`<audio>` + 5×`<video>`) параллельно.
2. `play()`/`load()` реализованы через независимый per-element `setInterval(fn, 50)`
   (`crates/js/src/audio_element.rs:236-301,495-531`, `POLL_MS = 50`), который на каждом тике
   читает `__lumen_audio_has_error(_handle)` — флаг, устанавливаемый ФОНОВЫМ потоком декодера
   в момент реального (не детерминированного) I/O.
3. Все активные таймеры страницы тикаются одним синхронным проходом
   (`_lumen_tick_timers`, `crates/js/src/shim/web_api_shim_mid_b.js:745-784`) — один вызов
   собирает все таймеры с истёкшим deadline и исполняет их в одной `V8Command::Run` job, после
   которой `drain_promise_rejections` (`crates/js/src/v8_runtime/promise_reject.rs:172-225`)
   один раз обрабатывает всё, что накопилось за эту job.
4. `testharness.js` фиксирует финальный статус по принципу «кто первый»:
   `error_handler` (`tests/wpt/resources/testharness.js:5048-5063`, вызывается на
   `unhandledrejection`) пишет `tests.status.status = ERROR` без проверки, что харнесс уже
   закончил; `notify_complete` (`:4037-4062`) пишет `status.status = OK` только если
   `status === null`.
5. Порядок «в каком именно `_lumen_tick_timers`-такте отказ `play()`-промиса элемента N
   долетит до `drain_promise_rejections`» относительно «когда синхронно завершится последний
   из 10 `async_test`» зависит от реального времени ответа фонового потока decoder'а —
   отсюда наблюдаемые ~`POLL_MS` (50 мс) различия между прогонами и итоговый флип
   ERROR/OK.

Реальные браузеры детектируют неподдерживаемый формат синхронно/быстро и детерминированно
(без настоящего decode-negotiation для заведомо неподдерживаемого содержимого) — poll-based
модель Lumen с независимыми per-element таймерами вносит недетерминизм там, где спека
подразумевает эффективно-синхронное поведение.

## Отличие от BUG-1022 (второй общий знаменатель, table-файлы)

Тем же срезом проверены `table-rows.html`/`table-insertRow.html`/`tHead.html` (файлы,
менявшие OK→ERROR между тремя `--check`-прогонами BUG-1022) — 5-10 изолированных повторов
каждого дали **детерминированный** `Test OK` (реальный дефект в сабтестах — то же семейство,
что [BUG-1032](BUG-1032-OPEN.md)/`GAP-TABLEIDL`), БЕЗ флипа статуса. Значит их OK→ERROR внутри
полного прогона BUG-1022 относится к другому, всё ещё не локализованному механизму того же
класса, что [BUG-1011](BUG-1011-OPEN.md) (накопление в длинном последовательном
`run_report.py`-прогоне), а НЕ к находке этого бага.

## Почему не фиксится точечно в этом срезе

Убрать гонку — не однострочная правка: нужно либо (а) сделать обнаружение ошибки декодера
event-driven (фоновый поток сам постит задачу в очередь таймеров/wakeup в момент возникновения
ошибки, а не полагается на случайный следующий 50-мс тик), либо (б) свести все per-element
media-poll таймеры одной страницы к единому детерминированному порядку обработки. Оба варианта —
изменение архитектуры доставки media-событий (`audio_element.rs`, вероятно `video_bindings.rs`),
а не точечный патч, и требуют отдельного проектирования/тестирования плеера, не наспех внутри
разбора BUG-1022.

## Воспроизведение

```
tests/wpt/.venv/bin/python tests/wpt/run_smoke.py --binary target/dev-release/lumen \
  /html/semantics/embedded-content/media-elements/ready-states/autoplay.html
# повторить 10 раз подряд — Test ERROR/Test OK чередуются при идентичных 10 FAIL-сабтестах
```

## Частичный митигейт 2026-09-07 (`crates/js/src/audio_element.rs`), гонка НЕ устранена

Изначально сделаны два точечных изменения без архитектурной переделки:

1. `play()` теперь проверяет `has_error` ДО входа в poll-цикл — если ошибка уже установлена,
   промис реджектится синхронно (`Promise.reject`), без единого тика `setInterval`.
2. `startLoad()`: poll-тело вынесено в именованную функцию `pollLoad()`, первый вызов идёт через
   `setTimeout(0)` сразу после `__lumen_audio_load()`, а не после ожидания полного `POLL_MS` (50мс).

Два юнит-теста (`play_rejects_immediately_when_error_already_set`,
`load_fires_error_on_first_tick_via_settimeout_zero`) зелёные, `cargo test -p lumen-js --lib
audio_element --features v8-backend` — 23/23.

**Живая проверка (2026-09-07, `run_smoke.py` × 9 повторов на пересобранном `dev-release`
бинаре, worktree `p3-work`, коммит `53874cbfe`) показала гонка НЕ устранена, только снижена:**

```
run 1: Test OK      run 4: Test OK      run 9:  Test ERROR
run 2: Test ERROR   run 5: Test ERROR   run 10: Test ERROR
run 3: Test OK      run 6: Test OK      run 11: Test ERROR
```

5 ERROR / 4 OK из 9 (было 9 ERROR / 1 OK из 10 на baseline) — заметное улучшение частоты, но
далеко не детерминированное `Test OK`. Диагноз §Механизм остаётся верным: `setTimeout(0)`
сокращает окно гонки (первый тик — на первом microtask-дренаже вместо случайного POLL_MS-такта),
но не устраняет его — все 10 `<audio>`/`<video>` элементов страницы всё ещё используют
независимые таймеры, разрешающиеся относительно реального времени ответа фонового
decoder-потока, а не в едином детерминированном порядке. Полноценный фикс по-прежнему требует
архитектурной переделки (§Почему не фиксится точечно), которую этот срез не сделал —
**статус возвращён в OPEN**, запись перенесена обратно из `BUGS-FIXED.md`.
