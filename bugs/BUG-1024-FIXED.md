# BUG-1024 — `html/canvas`: два `--check` подряд на свежем baseline дают два РАЗНЫХ, растущих набора регрессий

**Статус:** FIXED 2026-09-07 (P3)
**Заведён:** 2026-09-07 (P2, WPT-RUN-7 срез 21 — `html/canvas`, продолжение среза 20)
**Область:** не локализован. Регрессии рассыпаны по несвязанным подкаталогам
(`element/manual`, `element/fill-and-stroke-styles`, `offscreen/fill-and-stroke-styles`,
`offscreen/path-objects`, `element/path-objects`, `offscreen/transformations`,
`offscreen/compositing`, `element/pixel-manipulation`, `element/drawing-rectangles-to-the-canvas`
и др.) — никакого одного общего файла-знаменателя, в отличие от BUG-1022
**Владелец:** P1/P3 (после локализации)

## Симптом

`--update-expected --all --root html/canvas --recursive --processes 6` прошёл штатно
(**4 мин 32 с**, 3216/3308 harness OK, 945/5433 сабтестов, 2592 новых `.ini`). Два
последовательных `--check` на том же бинаре и том же свежезаписанном baseline, без
изменений между прогонами (`--processes 6` в обоих):

- прогон 1 (**4 мин 32 с**): **124 регрессии**, 166 unexpected pass, 0 other deviations
  (`.tmp/wpt-canvas-check1.stdout.log`);
- прогон 2 (**4 мин 18 с**): **140 регрессий**, 215 unexpected pass, 0 other deviations
  (`.tmp/wpt-canvas-check2.stdout.log`).

Растущий, а не колеблющийся счётчик (124 → 140), как в BUG-1022 (6 → 19 → 27), но здесь на
крупнейшей категории, где эта находка воспроизведена (3308 id против 2223 у BUG-1022).
Наборы регрессий пересекаются лишь частично: из 124+140 записей только 78 общие, 46
встречаются только в прогоне 1 (в прогоне 2 та же пара subtest вернулась к expected PASS),
62 — только в прогоне 2 (новые, отсутствовавшие в прогоне 1). Общий класс с BUG-1003/1004/
1005/1011/1022 подтверждён и на этой категории отдельным симптомом: заметная доля
`unexpected pass` (65 из 166 в прогоне 1) — это `offscreen/*.html`-тесты, у которых baseline
записал FAIL с `ReferenceError: OffscreenCanvas is not defined` (см. хвост
`.tmp/wpt-canvas-update.stdout.log`), а на следующем прогоне тот же тест на том же бинаре
неожиданно PASS — то есть глобал `OffscreenCanvas` виден не на каждом прогоне одного и того
же теста. Это может быть частным случаем общего механизма (расширяет гипотезу BUG-1022 о
утечке ресурсов конкретным наблюдаемым симптомом — гонка при установке глобала), а может
быть отдельной причиной; не разделено. Обратной связи «только offscreen» нет: 81 из 124
регрессий прогона 1 — вне `offscreen/`, в равно раскиданных `element/*`-подкаталогах.

Baseline `html/canvas` не закоммичен — `.ini`-файлы, записанные `--update-expected`, откачены
`git clean -fd tests/wpt/metadata/html/canvas/` до исходного (отсутствующего) состояния этим
же срезом.

## Почему это важно

Тот же класс находки, что [BUG-1003](BUG-1003-OPEN.md)/[BUG-1004](BUG-1004-OPEN.md)/
[BUG-1005](BUG-1005-OPEN.md)/[BUG-1011](BUG-1011-OPEN.md)/[BUG-1022](BUG-1022-OPEN.md)
(«N `--check` подряд без изменений между ними дают N разных наборов регрессий»), теперь на
самой крупной из проверенных категорий (3308 id) — при этом отличается от BUG-1022 тем, что
у растущего счётчика нет ни одного стабильного общего знаменателя-файла: регрессии рассеяны
по десятку несвязанных подкаталогов сразу, что говорит скорее в пользу ресурсной/нагрузочной
гипотезы (утечка портов/хендлов/памяти внутри одного `run_report.py`-прогона, растущая с
числом обработанных тестов), чем в пользу конкретного сломанного теста или общего хелпера.
Частный симптом `OffscreenCanvas is not defined` то появляющийся, то нет, на одном и том же
тесте между прогонами — самая конкретная зацепка на сегодня для локализации: указывает на
гонку при экспонировании глобала в контексте offscreen/worker, а не на общий износ ресурса,
раз оба класса симптомов (offscreen-специфичный и general element/*) присутствуют
одновременно.

## Воспроизведение

```
tests/wpt/.venv/bin/python tests/wpt/run_report.py \
  --binary target/dev-release/lumen --all --root html/canvas --recursive \
  --processes 6 --update-expected
tests/wpt/.venv/bin/python tests/wpt/run_report.py \
  --binary target/dev-release/lumen --all --root html/canvas --recursive \
  --processes 6 --check
# повторить --check несколько раз подряд без изменений между прогонами — набор регрессий
# не совпадает и растёт от раза к разу (124 → 140 на первых двух)
```

## Что не проверялось

- Третий/четвёртый `--check` подряд — есть только две точки, тренд опирается на минимальную
  выборку (как и в BUG-1022 на трёх точках).
- Изоляция симптома `OffscreenCanvas is not defined` конкретно (какой из ~65 задетых файлов
  ловит его детерминировано в изоляции через `run_smoke.py` в цикле, как это сделано для
  BUG-1011) — не делалось.
- Не сравнивалось поведение при последовательном (без `--processes`) прогоне, как это сделано
  в BUG-1011 для `html/rendering`, и не проверялось, растёт ли счётчик быстрее/медленнее при
  другом числе `--processes`.

## Не проверялось (категория)

Осталось крупных под-путей `html/*` без baseline: `semantics` 2223 (заблокирован BUG-1022),
`browsers` 759 (самый грязный), `rendering` 150 (заблокирован BUG-1011). `html/canvas`
(3308 id, крупнейший из проверенных) остаётся без baseline из-за этой находки.

## Срез P3 2026-09-07: root cause найден и исправлен

Живой прогон `--root html/canvas/offscreen/transformations --processes 1 --check
--log-raw` (изоляция от контента полной категории, но тот же класс симптома) поймал
реальный краш вместо флака. `--log-raw` показал последовательность:

```
thread 'lumen-v8' (22840) panicked at crates\js\src\v8_runtime\install\dom_core.rs:428:30:
BUG-986: NodeId 16 вне арены документа (len 16) — устаревший/чужой идентификатор, ...
thread 'lumen-v8' (22840) panicked at crates\js\src\v8_runtime\install\dom_core.rs:426:36:
called `Result::unwrap()` on an `Err` value: PoisonError { .. }
[повторяется 5 раз подряд, разные натив-вызовы]
thread 'main' (54260) panicked at crates\shell\src\app\about_to_wait.rs:1483:46:
called `Result::unwrap()` on an `Err` value: PoisonError { .. }
```

`_lumen_is_text_node` (`dom_core.rs:428`) вызывал паникующий `doc.get(nid)` вместо
bounds-checked `doc.try_get(nid)` — обычный BUG-986-класс дефект (устаревший/чужой
`NodeId`, переживший навигацию), который сам BUG-986 закрыл почти везде, но пропустил
несколько нативов в `dom_core.rs`. Паника на `doc.get()` разворачивается сквозь
`d.lock().unwrap()` захваченный `Arc<Mutex<Document>>`, `Mutex` остаётся poisoned, и
каждый следующий `.lock().unwrap()` того же документа (с любого потока — `lumen-v8`,
`main`) тоже паникует. Процесс не падает от одной паники (V8-граница ловит через
`catch_unwind`, `[JS native panic]` в логе), но каскад продолжается до полного разрыва
BiDi-сокета — снаружи выглядит как `os error 10054` посреди прогона. Разные тесты
успевают пройти до того, как каскад начнётся, в разных прогонах — отсюда «плавающие»
124→140 регрессии оригинальной находки: не флак тестов, а недетерминированный момент
краша.

**Фикс:** заменены все паникующие `doc.get(nid)`/`doc.get(root)`/`doc.get(c)` на
безопасный `doc.try_get(nid)` (уже существовал, введён при исходном BUG-986) в 10
нативах `crates/js/src/v8_runtime/install/dom_core.rs`: `_lumen_get_tag_name`,
`_lumen_get_local_name`, `_lumen_is_text_node`, `_lumen_is_comment_node`,
`_lumen_is_doctype`, `_lumen_get_document_doctype`, `_lumen_get_doctype_field`,
`_lumen_get_namespace_uri`, `_lumen_get_attr`, `_lumen_set_attr`/`_lumen_remove_attr`
(добавлен `contains_id` guard перед мутацией, т.к. они уже читают `.get_attr()` после
записи).

**Верификация:** `html/canvas/offscreen/transformations` (44 файла) под
`--processes 1 --check` — до фикса паника/каскад на ~24-й секунде (7-8-й тест из 44),
после фикса 44/44 harness OK за один непрерывный процесс (один и тот же PID на весь
прогон), 0 регрессий. `cargo test -p lumen-js --lib --features v8-backend`: 3537
passed, 1 failed (тот же [BUG-1030](BUG-1030-OPEN.md), подтверждено идентичным
на чистом main — не регрессия этого фикса); `cargo test -p lumen-dom --lib`: 292/292.
`cargo clippy --workspace --all-targets -- -D warnings`: чист.

**Не закрывает:** остаток категории `html/canvas` (3308 id) всё ещё без baseline —
этот срез устранил один конкретный краш-механизм на одной поддиректории, не прогнал
полный `--update-expected`/`--check` на всей категории (машина была занята
параллельными сессиями большую часть среза). Похожий паникующий `doc.get(nid)` может
существовать и в других install-файлах (`constructed_stylesheets.rs`, `net.rs`,
`platform.rs`, `stylesheets.rs`, `dom.rs`) — не проверено этим срезом, отдельная
задача при следующей находке той же формы.
