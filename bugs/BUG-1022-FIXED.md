# BUG-1022 — `html/semantics`: три `--check` подряд дают три РАЗНЫХ набора регрессий

**Статус:** FIXED 2026-09-25 (P3 — механизм части 3; остальные экземпляры класса → BUG-1011)
**Заведён:** 2026-09-07 (P2, WPT-RUN-7 срез 19 — `html/*` по под-путям, продолжение среза 18)
**Область:** не локализован. Общий знаменатель всех трёх прогонов —
`tabular-data/the-table-element/insertRow-method-02.html` (одни и те же 3 сабтеста FAIL
во всех трёх) плюс каждый раз ещё один, но РАЗНЫЙ файл в той же папке `the-table-element`
уходит harness OK→ERROR (`table-rows.html`, затем `table-insertRow.html`, затем
`tHead.html`); отдельно `embedded-content/media-elements/ready-states/autoplay.html`
(OK→ERROR) повторился во всех трёх прогонах, `forms/form-submission-0/*` — TIMEOUT-кластер,
плавающий по составу между прогонами 2 и 3, в прогоне 1 отсутствует
**Владелец:** P1/P3 (после локализации)

## Симптом

`--update-expected --all --root html/semantics --recursive` прошёл штатно (1739 новых
`.ini`, покрывающих все 2223 id категории). Три последовательных `--check` на том же
бинаре и том же свежезаписанном baseline, без изменений между прогонами
(`--processes 6` во всех трёх):

- прогон 1: **6 регрессий**, 5 unexpected pass, 26 other deviations
  (`.tmp/check-semantics-final-run1.log`);
- прогон 2: **19 регрессий**, 5 unexpected pass, 132 other deviations
  (`.tmp/check-semantics-final-run2.log`);
- прогон 3: **27 регрессий**, 7 unexpected pass, 94 other deviations
  (`.tmp/check-semantics-final.log`, PID пережил обрыв исходной сессии и достоверно
  досчитал до конца самостоятельно).

Ни один из трёх наборов не совпадает с другим целиком; общая часть — `insertRow-method-02.html`
(всегда одни и те же 3 сабтеста FAIL) и `autoplay.html` (всегда OK→ERROR), но количество и
состав TIMEOUT/ERROR-регрессий вокруг них растёт от прогона к прогону (6 → 19 → 27), а не
стабилизируется — не похоже на разовый флак одного теста.

Baseline `html/semantics` не закоммичен — `.ini`-файлы, записанные `--update-expected`,
откачены `git clean -fd tests/wpt/metadata/html/semantics/` до исходного (отсутствующего)
состояния этим же срезом.

## Почему это важно

Тот же класс находки, что [BUG-1003](BUG-1003-OPEN.md)/[BUG-1004](BUG-1004-OPEN.md)/
[BUG-1005](BUG-1005-OPEN.md)/[BUG-1011](BUG-1011-OPEN.md) («N `--check` подряд без изменений
между ними дают N разных наборов регрессий»), но на самой крупной пока категории, где это
воспроизведено (2223 id) — растущий, а не колеблющийся счётчик регрессий (6/19/27) наводит
на нагрузочную/ресурсную гипотезу (утечка портов/хендлов/памяти между последовательными
`--check`-прогонами внутри одного `run_report.py`-процесса, а не между процессами), но это
не проверялось целенаправленно.

## Воспроизведение

```
tests/wpt/.venv/bin/python tests/wpt/run_report.py \
  --binary target/dev-release/lumen --all --root html/semantics --recursive --update-expected
tests/wpt/.venv/bin/python tests/wpt/run_report.py \
  --binary target/dev-release/lumen --all --root html/semantics --recursive --check --processes 6
# повторить --check несколько раз подряд без изменений между прогонами — набор регрессий
# не совпадает и не убывает от раза к разу
```

## Что не проверялось

- Изоляция общего знаменателя (`insertRow-method-02.html`, `autoplay.html`) через
  `run_smoke.py` на одном id в цикле (10+ повторов) — не делалось.
- Гипотеза «счётчик растёт от прогона к прогону» не проверена на четвёртом/пятом
  прогоне — есть только три точки, тренд может быть совпадением малой выборки.
- Не сравнивалось поведение при последовательном (без `--processes`) прогоне, как это
  сделано в BUG-1011 для `html/rendering`.

## Не проверялось (категория)

Осталось крупных под-путей `html/*` без baseline: `canvas` 3308, `browsers` 759
(самый грязный). `html/semantics` (2223 id) остаётся без baseline из-за этой находки.

## Срез P3 2026-09-07

Изоляция обоих общих знаменателей через `run_smoke.py` на одном id, 5-10 повторов подряд
(`.tmp/bug1022-localize/run_isolated.sh`, скретч, не закоммичен).

**Часть 1 — `insertRow-method-02.html`:** 10/10 повторов дали идентичный результат — НЕ флак.
Корень: `HTMLTableElement`/`HTMLTableSectionElement`/`HTMLTableRowElement` не реализуют вовсе
ни одного специфичного для интерфейса IDL-члена (`insertRow`/`rows`/`cells`/`insertCell`/
`tHead`/`tBodies`/…). Заведён [BUG-1032](BUG-1032-OPEN.md) (ДОРАБОТКА → `GAP-TABLEIDL`).

**Часть 2 — `autoplay.html`:** В ИЗОЛЯЦИИ (без контекста полной категории) сам по себе флак —
10 повторов дали 9× `Test ERROR, expected OK` / 1× `Test OK`, при идентичном наборе 10/10
FAIL-сабтестов в обоих случаях. Механизм — гонка `error_handler`(`unhandledrejection`) vs
`notify_complete` в `testharness.js`, вызванная тем, что `play()`/`load()` в
`crates/js/src/audio_element.rs` детектируют ошибку декодера через независимый per-element
`setInterval(fn, 50)` (poll на фоновый поток), а не event-driven — относительный порядок
«отказ промиса N-го элемента» vs «завершение последнего из 10 `async_test`» зависит от
реального времени ответа decoder-потока. Не BiDi-транспорт: `crates/bidi-server` не
реализует ни одного log-события, гонка целиком в JS-документе теста. Заведён
[BUG-1033](BUG-1033-FIXED.md) — частичный митигейт (`setTimeout(0)` + синхронный reject на
уже установленной ошибке) снижает частоту гонки, но не устраняет её (5/9 ERROR живьём после
митигейта против 9/10 baseline) — полноценный фикс остаётся архитектурным (poll → event-driven).

**Часть 3 — `table-rows.html`/`table-insertRow.html`/`tHead.html`** (файлы, менявшие
OK→ERROR между тремя `--check`-прогонами): в изоляции детерминированы (`Test OK` 5-10/10,
реальный `GAP-TABLEIDL`-дефект, без флипа статуса) — значит их флип внутри полного прогона
НЕ объясняется находками частей 1-2, а относится к третьему, всё ещё не локализованному
механизму того же класса, что [BUG-1011](BUG-1011-OPEN.md) (накопление в длинном
последовательном `run_report.py`-прогоне по многим файлам).

**Остаётся нерешённым:** сам механизм части 3 (почему конкретно в контексте полного прогона,
а не в изоляции, у part-3 файлов дефект оборачивается ERROR, а не стабильным FAIL/OK), растущий
(не колеблющийся) TIMEOUT-кластер `forms/form-submission-0/*`, гипотеза «счётчик растёт от
прогона к прогону» (проверена лишь на трёх точках). Требует тяжёлых полнокорпусных
`--check`-прогонов на `html/semantics` (не выполнено этим срезом — бюджет сессии на фоновые
долгие процессы). `BUG-1022` остаётся `OPEN`.

## Ещё один экземпляр того же класса (2026-09-22, WPT-RUN-7 срез 54)

`referrer-policy/4K` (108 файлов, не `html/semantics`): baseline (`--update-expected`) дал
`top.http-rp/no-referrer-when-downgrade/fetch.http.html` как top-level `ERROR` (0/0,
без детального блока — тот же «голый ERROR» почерк, что part-3 выше), а
`top.http-rp/unsafe-url/fetch.http.html` и `top.meta/no-referrer-when-downgrade/fetch.http.html`
— как harness `OK` с 12 сабтестами `FAIL`. Первый же `--check` (тот же бинарь, без изменений
в движке между прогонами) перевернул все три: первый файл стал `OK`, два других — `ERROR`.
13 «регрессий» — сплошь сабтесты этих трёх файлов, ни одного за их пределами. Тот же почерк,
что часть 3 (`html/semantics`): OK↔ERROR флип на конкретных файлах между идентичными прогонами,
причина не локализована, другая категория корпуса — значит не специфично для
`html/semantics`/`tabular-data`, а действительно на уровне раннера/BiDi-навигации, как
предполагает заголовок этого бага. Не расследовано глубже в рамках среза 54 — та же причина,
что здесь: полнокорпусный прогон `referrer-policy` (1390 файлов) доминирован DNS-таймаутом на
`www1.localhost` (см. `docs/tasks/p2-test-track.md#test-3-срез-54`) и не оставляет бюджета на
повторные `--check` ради локализации флапа.

## Ещё один экземпляр того же класса (2026-09-22, WPT-RUN-7 срез 55)

`fetch` (906 id): два независимых чистых `--check` (без инфраструктурного сбоя раннера,
см. `docs/tasks/p2-test-track.md#test-3-срез-55-2026-09-22`) дали 13 и 34 регрессии
соответственно, пересекающиеся лишь частично — `fetch/orb/tentative/unknown-mime-type.sub.any.html`
(`.worker.html` тоже) и `fetch/orb/tentative/compressed-image-sniffing.sub.html` (`OK`→`TIMEOUT`)
повторились в обоих прогонах, но `fetch/orb/tentative/known-mime-type.sub.any.html` (25 сабтестов
`PASS`→`NOTRUN`/`TIMEOUT`, самый крупный кластер второго прогона) и
`fetch/metadata/generated/svg-image.sub.html` отсутствовали в первом прогоне вовсе, а
`fetch/metadata/generated/element-frame.sub.html`/`element-script.sub.html` регрессировали на
РАЗНЫХ сабтестах (`sec-fetch-dest`/`sec-fetch-site` — разные заголовки) между прогонами. Тот же
почерк: TIMEOUT/NOTRUN-кластер вокруг `fetch/orb/tentative/*` и `fetch/metadata/generated/*`
(оба — генерируемые id с большим числом параллельных cross-origin подзапросов на файл, как и
`referrer-policy/4K*` выше) нестабилен между идентичными прогонами; в отличие от этого,
`fetch/api/redirect/redirect-schemes.any.html` [`redirects 1`] регрессировал ОДИНАКОВО во всех
трёх прогонах (включая невалидный из-за обрыва раннера) и трижды воспроизведён изолированно
`run_smoke.py` — не отнесён к этому классу, заведён отдельно как
[BUG-1098](BUG-1098-OPEN.md) (детерминированный дефект, не флап). Baseline `fetch` принят как
записан первым `--update-expected`; TIMEOUT/NOTRUN-кластер `orb/tentative`/`metadata/generated`
не перегенерирован — тот же случай, что `referrer-policy/4K*` часть 3, требует отдельной
локализации вне бюджета этого среза.

## Ещё один экземпляр того же класса (2026-09-23, WPT-RUN-7 срез 59)

`signed-exchange` (перегенерация после [BUG-1069](BUG-1069-FIXED.md)): `--update-expected`
дал 20 файлов `reporting/` с top-level `expected: TIMEOUT` (домен `not-web-platform.test`,
диагностирован в срезе 40 — резолвится только через hosts-запись, WPT-RUN-10). Четыре
`--check` подряд на том же бинаре и baseline, без изменений между прогонами: прогон 1 —
2 регрессии (`OK`→`TIMEOUT` на двух других `reporting/`-файлах) + 1 unexpected pass; прогон
2 (после сужения прогона 1) — 0 регрессий + 2 unexpected pass на ДРУГИХ двух файлах; прогон
3 — 0 регрессий + 1 unexpected pass на ещё одном файле; прогон 4 — 0 регрессий + 5 unexpected
pass сразу на пяти файлах (включая `subresource/sxg-subresource-header-integrity-mismatch`,
до этого не всплывавший). Растущий, не убывающий счётчик (1→2→1→5) — тот же почерк, что часть
3 (`html/semantics`) и `referrer-policy`/`fetch` выше, но здесь все 20 подозрительных файлов
делят один и тот же внешний домен, что впервые указывает на конкретный механизм: DNS-резолюция/
кеш `not-web-platform.test` состязается с фиксированным таймаутом теста, и то, успевает ли она
уложиться, меняется от прогона к прогону без изменений на стороне движка. Правки-сужения
(`expected: [OK, TIMEOUT]`), сделанные по итогам прогонов 1–3, откачены — baseline принят таким,
каким его записал исходный `--update-expected`, дальнейшее сужение не проводилось (тот же выбор,
что для `fetch` в срезе 55).

## Ещё один экземпляр того же класса (2026-09-23, WPT-RUN-7 срез 61)

`shared-storage` (90 файлов): `--update-expected` дал 47/90 harness OK, 24/221 сабтестов,
86 `.ini` записано. Три `--check` подряд на том же бинаре и baseline, без изменений между
прогонами: прогон 1 — 5 регрессий + 3 unexpected pass + 1 status-change; прогон 2 —
4 регрессии + 3 unexpected pass + 1 status-change; прогон 3 — 4 регрессии + 3 unexpected
pass + 1 status-change. Три набора регрессий пересекаются частично, но не совпадают —
`shared-storage-writable-service-worker-img.tentative.https.sub.html` (OK→TIMEOUT на
сабтесте «same origin img» + status-change NOTRUN на «cross origin img») и
`shared-storage-permissions-policy-none`/`select-url-permissions-policy-none`
(FAIL→TIMEOUT) держатся в двух прогонах из трёх, но третий регрессирующий файл каждый раз
другой (`cross-origin-create-worklet-credentials-omit`, `shared-storage-permissions-policy-self`,
`shared-storage-writable-setters` — по одному на прогон). Unexpected-pass тройка тоже плавает
по конкретным `*-permissions-policy-*`/`cross-origin-create-worklet-credentials-*` файлам, но
неизменно из одного и того же семейства (permissions-policy TIMEOUT↔PASS,
create-worklet-credentials TIMEOUT↔OK). Тот же почерк, что `fetch`/`signed-exchange` выше: все
подозрительные файлы используют `SharedStorageWorklet`/service worker/cross-origin iframe —
конструкции с несколькими параллельными подключениями и фиксированным таймаутом теста, где
исход гонки меняется от прогона к прогону без изменений на стороне движка. Baseline принят
таким, каким его записал исходный `--update-expected`, дальнейшее сужение не проводилось (тот
же выбор, что для `fetch`/`signed-exchange`).

## Ещё один экземпляр того же класса (2026-09-23, WPT-RUN-7 срез 63)

`fetch`, перегенерирован повторно после [BUG-1069](BUG-1069-FIXED.md) (первая регенерация была
в срезе 55, ДО фикса). Три `--check` подряд на том же бинаре и baseline: 76/102/60 регрессий,
35 уникальных файлов суммарно, попарное пересечение 11–16 из 25–30 — без общего знаменателя.
Кластеры: `fetch/metadata/generated/*` (13 файлов — Fetch Metadata Request Headers на iframe/
worker/serviceworker), `fetch/metadata/*` верхнего уровня (9), `fetch/orb/tentative/*` (4, в т.ч.
`nosniff.sub.any.html`, пойманный только отдельным verify-прогоном — 14 регрессий на нём одном,
ни разу не всплывших в основных трёх `--check`), `fetch/corb/*` (1), `fetch/security/
dangling-markup/*` (2), `fetch/stale-while-revalidate/*` (1). Все — service-worker/worklet/
iframe-конструкции с несколькими параллельными подключениями и фиксированным таймаутом, тот же
механизм, что `shared-storage`/`signed-exchange` выше. Baseline не откачен, не сужен. Отдельно
(НЕ этот класс — детерминированная, не диффузная ошибка baseline) 4 файла регрессировали
identично во всех трёх прогонах и были исправлены вручную под воспроизводимое значение:
`request-cache-only-if-cached.any.sharedworker.html`, `img-mime-types-coverage.tentative.
sub.html` (`fetch/corb`), `status.sub.any.worker.html` (`fetch/orb/tentative`), `style.https.
sub.html` — детали в `docs/tasks/p2-test-track.md#test-3-срез-63-2026-09-23`.

## Фикс P3 2026-09-25 — механизм части 3: упавший браузер не перезапускался

**Корень — в раннере, не в движке.** `run_smoke.py` запускает wptrunner с
`--no-restart-on-unexpected`, поэтому Lumen между тестами перезапускается только по
статусам `CRASH`/`EXTERNAL-TIMEOUT`/`INTERNAL-ERROR` (`testrunner.py`,
`restart_before_next`). Когда тест *убивает* браузер, `executorlumen.py` отдавал обычный
`ERROR`:

* `LumenBidiProtocol.is_alive()` проверял `session.transport is not None`, а
  `BidiSession` не обнуляет `transport` при обрыве сокета — после смерти процесса
  метод продолжал отвечать `True`, так что и `TimedRunner` не мог переквалифицировать
  результат в `CRASH`;
* `do_test` пропускал `UnknownErrorException("WebSocket connection closed")` из
  `browsingContext.navigate` как `ExecutorException("ERROR", …)`.

Воркер оставался с мёртвой сессией, и **следующий** тест в его очереди падал на
`_reset_and_mark` тем же `ConnectionClosedError` — голый `ERROR` без сабтестов. Какой
файл окажется «следующим», решает шардинг `--processes 6`, поэтому каждый `--check`
переворачивал OK→ERROR другой невиновный файл — ровно почерк части 3 (`table-rows.html`,
затем `table-insertRow.html`, затем `tHead.html` — все соседи по `tabular-data`).

**Кто убивает браузер.** `processing-model-1/span-limits.html` делает
`tbody.innerHTML += "<tr><td>" × 65532`. Lumen разбирает фрагмент в режиме `in body`
вместо `in table body` и вкладывает каждую строку в ячейку предыдущей — DOM-цепочка
глубиной ~131 000, `Maximum call stack size exceeded` и
`thread 'lumen-pipeline' has overflowed its stack` → abort. Заведён
[BUG-1155](BUG-1155-OPEN.md). Воспроизведение живым прогоном 2026-09-25
(`run_report.py --all --root html/semantics/tabular-data --recursive --processes 6`,
четыре прогона до фикса): в двух из четырёх вместе с `span-limits.html` упал сосед —
`caption-methods.html` (прогон 1) и после первой половины фикса `sectionRowIndex.html`/
`rows.html` (прогоны 3–4).

Второй облик той же утечки нашёлся в прогонах 3–4: `span-limits.html` иногда не роняет
процесс сразу, а вешает цикл автоматизации (`navigate: automation command timed out`,
`crates/driver/src/automation.rs`) — сокет ещё жив, процесс умирает уже на навигации
соседа.

**Фикс** (`tools/wptrunner/wptrunner/executors/executorlumen.py`):

* `is_alive()` смотрит на `transport.read_message_task` — задача-читатель
  завершается именно на `ConnectionClosed`;
* `do_test` переводит исключение в `CRASH`, если после него сессия мертва, и в
  `EXTERNAL-TIMEOUT`, если сервер ответил `automation command timed out`. Оба статуса
  перезапускают браузер перед следующим тестом.

**Проверка.** Два прогона `tabular-data` после фикса — 28/29 harness OK, 142/152
сабтестов, единственный не-OK — сам `span-limits.html` (`TIMEOUT`); у соседей ERROR
больше нет. Регрессионная проверка
[`tests/wpt/verify_bug1022_crash_restart.py`](../tests/wpt/verify_bug1022_crash_restart.py)
убивает `lumen --bidi-port` посреди настоящего `do_test` и требует `CRASH` и
`is_alive() == False`; на коде до фикса она падает (`WebSocket connection closed`
вылетает как ошибка, а не `CRASH`). Полный `--check` по `html/semantics` (2223 id,
40+ мин) не повторялся; частичный прогон 2026-09-25 (665 файлов до ручной остановки)
не дал ни одного `WebSocket connection closed`/`CRASH` — все 33 `ERROR` содержательные
(`addTextTrack is not a function`, `originSameOrigin is not defined`, селекторы `*|`).

**Что закрыто и что нет.** Закрыт механизм части 3 — «чужой» OK→ERROR после падения
браузера. Дописанные ниже экземпляры (`referrer-policy/4K` — голый `ERROR`, похоже на тот
же механизм, но не перепроверялся; `fetch`/`signed-exchange`/`shared-storage` и
TIMEOUT-кластер `forms/form-submission-0/*` — гонки таймаутов самих тестов, а не
заражение соседа) относятся к классу [BUG-1011](BUG-1011-OPEN.md) и ведутся там.
