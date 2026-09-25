# BUG-1169 — под тяжёлой нагрузкой раннер wptrunner не успевает в `init_timeout` 30 с, прогон теряет всю очередь

**Статус:** OPEN
**Компонент:** WPT-харнесс (`tools/wptrunner/wptrunner/testrunner.py` — `TestRunnerManager.init`/`init_timeout`;
`tools/wptrunner/wptrunner/browsers/lumen.py` — `LumenBrowser` не переопределяет `init_timeout`, берётся
`WebDriverBrowser.init_timeout = 30` из `browsers/base.py:104`)
**Найден:** 2026-09-25, P6, BUG-1073 срез 6 (прогон `fix1`)

## Симптом

`run_report.py --all --root pointerevents --recursive --processes 4 --check` параллельно с холодной
сборкой `cargo test -p lumen-shell --no-run -j 8` (свой `CARGO_TARGET_DIR`, без sccache), бинарь
`dev-release` с правкой BUG-1073 срез 6. Все четыре `lumen --bidi-port` поднялись (`[bidi] token`,
`Webdriver started successfully`, выбор бэкенда за 10.5–13.2 с), но ни один тест не начался:

```
 4:56.80 INFO Starting runner
 5:34.62 WARNING Forcibly terminating runner process        (через ~38 с)
 5:40.26 ERROR Failed to stop either the runner or the browser process
 ...
 9:29.13 CRITICAL Max restarts exceeded
```

23 `Forcibly terminating runner process`, 0 `TEST_START`, итог — 258 `MISSING`
(`check: 258 regression(s)`). `did not print [bidi] token` и `os error 10048` — 0, то есть это уже не
BUG-1073.

## Что известно

- В том же прогоне под более лёгкой нагрузкой (отладочный `--log-raw-level=debug`, 24 id) от
  `Test runner started` (порождён `mp.Process`) до `Executor setup` в дочернем процессе — 14 с
  при уже слушающем `lumen`. Под тяжёлой фазой сборки это, по всей видимости, выходит за 30 с.
- Что именно занимает время — запуск и импорты дочернего Python (`spawn`) или `session.new` через
  BiDi — **не разделено**. Голая проба `session.new` + `getTree` на четырёх одновременно стартовавших
  `lumen` отвечала за 0.1–0.6 с после TCP-подключения (без сборки рядом).
- Для сравнения: `init_timeout` у Chrome в том же wptrunner — 65 с, у Firefox — 70 с.

## Ожидание

Прогон под нагрузкой замедляется, но не теряет очередь целиком из-за старта раннера.

## Что сделать

Разделить время старта раннера по фазам (spawn → импорт → `connect` → `session.new` → `getTree`) под
той же нагрузкой, потом решать: поднять `init_timeout` у `LumenBrowser` или убрать то, что медленно.
