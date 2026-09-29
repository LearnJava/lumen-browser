# BUG-1004 — `close-watcher/user-activation/*`: набор упавших/прошедших файлов меняется от прогона к прогону

**Статус:** OPEN
**Заведён:** 2026-09-05 (P2, WPT-RUN-7 срез 6 — перегенерация expectations baseline после
фикса выборки среза 4)
**Область:** не локализован. Общая точка всех задетых тестов —
`close-watcher/user-activation/` (симуляция/потребление user activation перед созданием
`CloseWatcher`), сами файлы разные в разных прогонах
**Владелец:** P1/P3 (после локализации)

## Симптом

`--update-expected --all --root close-watcher --recursive`, затем два независимых
`--check`-прогона подряд на том же бинаре без пересборки:

- Прогон 1: 4 регрессии (`n-destroy-n.html?dialog`, `n-destroy-n.html?CloseWatcher`
  подтест, `n.html?dialog` подтест, `nn-CloseWatcher.html`) + 2 неожиданных PASS
  (`n-destroy-n.html?CloseWatcher`, `n.html?dialog` — те же id, что и в регрессиях, другая
  грань).
- Прогон 2 (без каких-либо изменений между прогонами): 2 регрессии
  (`n-closerequest-n.html?dialog`, `n-destroy-n.html?CloseWatcher` подтест — другой набор,
  чем в прогоне 1) + 1 неожиданный PASS.

Ни один набор не повторился между прогонами — не «одна и та же группа всегда красная»,
а перетасовка какого именно файла в `user-activation/` не повезёт в этом запуске.

## Почему это важно

Тот же класс невоспроизводимого подтестового baseline, что [BUG-999](BUG-999-OPEN.md) и
[BUG-1003](BUG-1003-FIXED.md): `--check` красится без единого движкового изменения между
запусками. `close-watcher` **не перегенерирован** этим срезом (изменения к
`tests/wpt/metadata/close-watcher/` полностью откачены `git checkout --`) — baseline
остаётся в состоянии среза 4 (17 категорий пакета не включали `close-watcher`; это один из
21 категории долга среза 4/5), перегенерация отложена до локализации причины.

## Воспроизведение

```
LUMEN_PROFILE=dev-release python tests/wpt/run_report.py \
  --binary target/dev-release/lumen --all --root close-watcher --recursive --update-expected
LUMEN_PROFILE=dev-release python tests/wpt/run_report.py \
  --binary target/dev-release/lumen --all --root close-watcher --recursive --check
```

Повторить `--check` 3–5 раз подряд — набор регрессий/unexpected-PASS меняется каждый раз,
всегда внутри `user-activation/`.

## Гипотеза (непроверенная)

`user-activation/*`-тесты по конструкции зависят от того, было ли у документа реальное
"transient activation" на момент вызова `CloseWatcher()` — если у Lumen это состояние
утекает или гоняется в реальном времени между тестами одного процесса (а не строго
сбрасывается на каждую навигацию), результат конкретного теста зависит от порядка/тайминга
соседних тестов в той же категории. Не подтверждено — не проверялось, действительно ли
`document.hasBeenActive`/эквивалент компонент сбрасывается на `browsingContext.navigate`.

## Срез 1 (P6, 2026-09-29)

Найдена причина **части** нестабильности: wptrunner для `test_driver.click`/`send_keys`
строит селектор вида `:root > *|body:nth-child(2)`, а парсер селекторов не знал префикс
пространства имён `*|` / `|` → `SyntaxError: ... is not a valid selector`, тест уходил в
`ERROR`. Починено в `parse_simple_selector` (префикс отбрасывается, `@namespace` не
поддержан); юнит-тест `namespace_prefix_any_or_none_is_accepted`. После фикса
`--check` по `close-watcher/user-activation` — 0 регрессий; убран устаревший
`nnn-popovers.html.ini` (UNEXPECTED-PASS).

**Остаток (bug остаётся OPEN):** в одном из ~4 массовых `--check`-прогонов 1–2 теста
(`y.html?dialog`, `ny-activate-preventDefault.html?CloseWatcher`) дают `CRASH`, тогда как
по отдельности проходят стабильно (4/4). Гипотеза: падение/перезапуск окна между тестами
одного процесса, а не user activation. Не локализовано.
