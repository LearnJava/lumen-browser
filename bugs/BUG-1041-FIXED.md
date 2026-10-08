# BUG-1041 — Web Animations: `finish`/`cancel` vs `requestAnimationFrame` — гонка, порядок не гарантирован

**Статус:** FIXED 2026-09-30 (P6)
**Заведён:** 2026-09-08 (P2, WPT-RUN-7 срез 32 — `--update-expected` для `web-animations`)
**Область:** не локализовано в Rust-коде. Кандидат — диспетчеризация событий Web Animations
(`finish`/`cancel`) относительно очереди `requestAnimationFrame`, вероятно в шиме
`crates/js/src/shim/*.js` или в native-стороне `Animation.finish()`/`.cancel()`
**Владелец:** P1/P3 (после локализации)

## Симптом

`/web-animations/timing-model/timelines/update-and-send-events.html` содержит два
симметричных подтеста:

- «Fires cancel event before requestAnimationFrame»
- «Fires finish event before requestAnimationFrame»

Спека требует, чтобы оба события (`cancel` и `finish`) диспетчеризовались как часть
микротаск-чекпоинта процедуры «update animations and send events», то есть **до**
следующего колбэка `requestAnimationFrame`.

Шесть последовательных прогонов той же категории на одном бинаре (2×`--update-expected`,
4×`--check`) дали для этой пары подтестов **три разных комбинации** PASS/FAIL, при этом
всегда ровно один из двух — PASS, другой — FAIL (никогда оба одинаково):

| прогон | cancel | finish |
|---|---|---|
| update-expected #1 | FAIL | PASS |
| check #1–#3 (3 подряд, идентичны друг другу) | PASS | FAIL |
| update-expected #2 | FAIL | PASS |
| check #4 | FAIL | PASS |
| check #5 | FAIL | **PASS** (сообщено как info: unexpected PASS на `finish`) |
| check #6 | **PASS** (info: unexpected PASS на `cancel`) | FAIL |

Значит это не «холодный старт первого прогона» (гипотеза, опровергнутая вторым
`--update-expected`, который воспроизвёл ИСХОДНОЕ состояние), а настоящая, воспроизводимая
на живом движке гонка между диспетчеризацией одного из двух событий и постановкой
rAF-колбэка в очередь — какое из двух «успевает», решает что-то недетерминированное
(вероятно порядок обработки процессов wptrunner/загрузка машины, но не исключена и
внутридвижковая гонка, независимая от внешней нагрузки).

Контрольный `--check` после мержа `origin/main` (2026-09-08, тот же срез) снова дал info:
unexpected PASS на `cancel` — тот же флип, ожидаемо, гейт не покраснел (см. §Обход).

## Почему это важно

Не тот же класс, что [BUG-999](BUG-999-FIXED.md)/[BUG-1003](BUG-1003-FIXED.md)/
[BUG-1004](BUG-1004-CANNOT-REPRODUCE.md)/[BUG-1005](BUG-1005-FIXED.md)/[BUG-1038](BUG-1038-FIXED.md) —
там весь `--check` невоспроизводим (плавает произвольно, включая TIMEOUT на других файлах).
Здесь ровно один файл, ровно два симметричных подтеста, и всегда PASS+FAIL (никогда
FAIL+FAIL/PASS+PASS) — сильный сигнал, что оба подтеста меряют один и тот же
недетерминированный момент времени (вероятно оба используют один `requestAnimationFrame`-
колбэк или таймер как реперную точку, и то, какое из двух animation-событий успевает до
него, решает гонка).

**Обход для гейта (срез 32):** `tests/wpt/metadata/web-animations/timing-model/timelines/
update-and-send-events.html.ini` — `expected: [FAIL, PASS]` для обоих подтестов (`FAIL`
первым). `expectations.py`'s `classify_one` сравнивает актуальный статус только с ПЕРВЫМ
элементом списка (`SUBTEST_GOOD_STATUSES = {"PASS"}`; `was_good = expected[0] in
GOOD_STATUSES`) — второй/третий элемент списка в отчёте `wptreport` не подавляет ключ
`"expected"` сам по себе, он просто не читается этим скриптом вовсе. Поставив `FAIL`
первым, любой флип читается максимум как «unexpected PASS — narrow expectations» (info,
не гейтит), никогда как REGRESSION. Тот же приём уже использован в
`css/css-position/sticky/position-sticky-scrolled-remove-sibling.html.ini` — но там это
осознанный выбор автора, а не задокументированный контракт `expectations.py`;
стоит явно описать это в `docs/tasks/p2-test-track.md`, чтобы следующий срез не наступал
на ту же путаницу заново.

## Воспроизведение

```
LUMEN_PROFILE=dev-release python tests/wpt/run_report.py \
  --binary target/dev-release/lumen --all --root web-animations --recursive --check
```

Запустить 2+ раза подряд на одном бинаре без пересборки — пара подтестов
`update-and-send-events.html` [Fires cancel/finish event before requestAnimationFrame]
меняется местами (PASS↔FAIL), сумма всегда «один PASS, один FAIL».

## Что не проверялось

Не изолировано до конкретного места в шиме/нативном коде, диспетчерящем
`AnimationPlaybackEvent`. Не проверено на `run_smoke.py` с этим одним id в изоляции (без
остальных 138 файлов категории, параллельно нагружающих воркеры `--processes 6`) — не
исключено, что гонка зависит именно от параллельной нагрузки, а не является чисто
внутридвижковой.

## Исправление (2026-09-30, P6)

Причина: событие `finish`/`cancel` уходило только задачей в `_lumen_timers`, а
`_lumen_run_raf_callbacks` не смотрел в эту очередь — кто из двух (задача или кадр)
выполнится первым, решал момент тика. Спека («update animations and send events»)
требует событий раньше rAF того же кадра. Теперь `Animation._fire` кладёт запись в
`_wa_pending_events`; `_lumen_run_raf_callbacks` сначала сливает её, задача-таймер
диспетчеризует, только если запись ещё не ушла. Тест —
`animation_playback_events_precede_raf_callbacks_bug1041`.

Не проверено: WPT-категория `web-animations` не прогонялась (вендор-каталог в слоте
отсутствует), `.ini` с `expected: [FAIL, PASS]` оставлен — P2 сузит при следующем срезе.
