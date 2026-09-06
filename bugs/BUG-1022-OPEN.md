# BUG-1022 — `html/semantics`: три `--check` подряд дают три РАЗНЫХ набора регрессий

**Статус:** OPEN
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
