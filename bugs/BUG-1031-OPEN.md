# BUG-1031 — `soft-navigation-heuristics`: два `--check` подряд на свежем baseline дают два непересекающихся набора регрессий

**Статус:** OPEN
**Заведён:** 2026-09-07 (P2, WPT-RUN-7 срез 25)
**Область:** не локализован — регрессии в обоих прогонах задели разные файлы/сабтесты
(прогон 1: `smoke/tentative/image-src-change.html`, `smoke/tentative/video-src-change.html`;
прогон 2: `icp/tentative/text-node-modification-does-not-affect-siblings.html`,
`navigation-api-hash.tentative.html`), общего файла-знаменателя между прогонами нет
**Владелец:** P1/P3 (после локализации)

## Симптом

`--update-expected --all --root soft-navigation-heuristics --recursive --processes=6`
прошёл штатно за 1:12 (21/94 harness OK, 0/49 сабтестов — категория экспериментальная,
большинство тестов TIMEOUT/ERROR даже в baseline, это ожидаемо и не сама находка). Два
последовательных `--check` на том же бинаре и том же свежезаписанном baseline, без
изменений между прогонами (`--processes=6` в обоих):

- прогон 1: **21 регрессия**, 6 unexpected pass (narrow expectations), 0 other deviations;
- прогон 2: **15 регрессий**, 7 unexpected pass (narrow expectations), 0 other deviations.

Полные логи не сохранялись отдельным файлом (только хвост stdout), поэтому точное
пересечение множеств регрессий между прогонами не подсчитано — но ни один из видимых в
хвосте регрессий прогона 1 (`image-src-change.html`, `video-src-change.html`) не повторился
в хвосте прогона 2, что уже отличается от растущего/пересекающегося счётчика BUG-1022/1024
(там был явный общий знаменатель-файл). Baseline `soft-navigation-heuristics` не
закоммичен — `.ini`-файлы, записанные `--update-expected`, откачены `git clean -fd
tests/wpt/metadata/soft-navigation-heuristics/` до исходного (отсутствующего) состояния
этим же срезом.

## Почему это важно

Тот же класс находки, что [BUG-1003](BUG-1003-OPEN.md)/[BUG-1004](BUG-1004-OPEN.md)/
[BUG-1005](BUG-1005-OPEN.md)/[BUG-1011](BUG-1011-OPEN.md)/[BUG-1022](BUG-1022-OPEN.md)/
[BUG-1024](BUG-1024-FIXED.md) («N `--check` подряд без изменений между ними дают N разных
наборов регрессий»), теперь на маленькой (94 id) и в основном не проходящей категории —
показывает, что механизм не зависит ни от размера категории (BUG-1003/1004 тоже были
небольшими), ни от доли PASS в baseline (здесь harness OK всего 21/94, большая часть
baseline и так уже FAIL/ERROR/TIMEOUT). Расширяет выборку категорий, задетых этим классом,
но новых зацепок к локализации не добавляет.

## Воспроизведение

```
tests/wpt/.venv/bin/python tests/wpt/run_report.py \
  --binary target/dev-release/lumen --all --root soft-navigation-heuristics --recursive \
  --processes=6 --update-expected
tests/wpt/.venv/bin/python tests/wpt/run_report.py \
  --binary target/dev-release/lumen --all --root soft-navigation-heuristics --recursive \
  --processes=6 --check
# повторить --check ещё раз без изменений между прогонами — набор регрессий не совпадает
```

## Что не проверялось

- Логи `--check` не сохранялись через `--log-raw`/redirect в файл — только хвост stdout,
  поэтому точный overlap между прогонами (как в BUG-1022/1024) не подсчитан.
- Третий `--check` подряд — есть только две точки.
- Изоляция конкретных задетых файлов через `run_smoke.py` в цикле (как для BUG-1011) — не
  делалась.

## Срез P3 2026-09-08 — механизм BUG-1024 (каскадная паника → отравленный Mutex) проверен и отклонён

Гипотеза: та же дыра, что чинил [BUG-1024](BUG-1024-FIXED.md) (непроверенный `doc.get(nid)` в
JS-натив, доступном напрямую со страницы, паникует на чужом/устаревшем `NodeId`, отравляя
`Mutex<Document>` — все последующие `.lock().unwrap()` того же документа тоже паникуют
каскадом, что и выглядит как «плавающий» набор регрессий между прогонами). Аудит нашёл и
починил шесть ещё непроверенных путей этого же класса в `dom_core.rs`/`platform.rs` —
[BUG-1036](BUG-1036-FIXED.md).

**Результат: гипотеза отклонена.** Тот же бинарь (пересобран с фиксом BUG-1036) на том же
репро — `--update-expected`, затем два `--check` подряд — дал 18 и 21 регрессию
соответственно, непересекающиеся так же, как до фикса. Симптом воспроизводится один в один —
причина BUG-1031 не эта дыра, а другой, всё ещё не найденный механизм. Остаётся `OPEN`,
владелец не изменился.
