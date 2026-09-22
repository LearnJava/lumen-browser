# BUG-1072 — `websockets/back-forward-cache-closes-open-websocket-connection.tentative.window.html` вешает весь процесс браузера

**Статус:** OPEN
**Тип:** зависание процесса — кандидат в механизм `hung-browser`.
**Заведён:** 2026-09-20 (P6, повторный прогон `run_report.py --all --root websockets --recursive` после GAP-WSASYNC срезов 1-4)
**Область:** не локализовано — либо bfcache-путь при живом WebSocket-соединении (`crates/shell/src/lumen/bfcache.rs`), либо сам WebSocket на закрытии страницы. Не проверялось напрямую, только через wptrunner.
**Владелец:** P3 (триаж по [docs/probe-method.md §8](../docs/probe-method.md)).

## Симптом

`run_report.py --all --root websockets --recursive` (binary собран на HEAD `9e384ca01`, GAP-WSASYNC срезы 1-4 включены): на файле
`websockets/back-forward-cache-closes-open-websocket-connection.tentative.window.html` тест уходит в `TIMEOUT` через ~34 с ожидания
результатов `testharnessreport.js`, а затем сам процесс `lumen.exe` не завершается по команде wptrunner — mozprocess:

```
wptrunner.executors.base.ExecutorException: ('TIMEOUT', 'Timed out waiting for testharnessreport.js results: .../websockets/back-forward-cache-closes-open-websocket-connection.tentative.window.html')
...
OSError: IO Completion Port failed to signal process shutdown
...
RuntimeError: lumen --bidi-port did not print [bidi] token
```

После первого краха релонч браузера падает ещё дважды подряд с той же ошибкой (порт `--bidi-port` не освобождается вовремя), и весь прогон
обрывается на 134/331 harness OK вместо ожидаемых ~342/515 (числа предыдущего прогона от 2026-08-18, `docs/wpt-status.md` строка `websockets`) —
то есть один тест уносит не только себя, а хвост всего прогона (тот же класс, что уже описан у `hung-browser` в BUG-835/BUG-988).

## Ожидание

Тест либо проходит/падает по существу за штатный таймаут, либо помечается `TIMEOUT` штатно — но браузер должен завершаться по сигналу
закрытия/kill в разумное время, не блокируя относящийся к нему `--bidi-port`.

## Не проверялось

- Изолированный repro вне wptrunner (голый BiDi-клиент, `docs/probe-method.md` §про голую пробу).
- Связь с bfcache: тест называется `back-forward-cache-closes-open-websocket-connection`, но не проверено, действительно ли зависание в
  bfcache-пути, а не в закрытии WebSocket на смене документа.
- Воспроизводится ли на срезах 1-3 GAP-WSASYNC (то есть до/после отменяемого хэндшейка среза 4) — не бисекциировано.

## Заметка среза 44 WPT-RUN-7 (2026-09-21)

Тот же хвост (`IO Completion Port failed to signal process shutdown` → `did not print [bidi] token` → обрыв всего прогона) наблюдался и на `pointerevents`, но с другим триггером — падение `lumen.exe` при старте (`present=WHITE`, паника `wgpu … Invalid surface`), а не зависший тест: [BUG-1073](BUG-1073-OPEN.md). Общая часть — `TestRunnerManager` не переживает три неудачных релонча подряд.

## Заметка среза 55 WPT-RUN-7 (2026-09-22)

Один из трёх `--check`-прогонов `fetch` (906 id, `--processes 7`) оборвался тем же
`RuntimeError: lumen --bidi-port did not print [bidi] token`, `CRITICAL Tests left in the queue:
... and 110 others` — 111 файлов из 906 остались без результата (`MISSING`), прогон
признан невалидным и отброшен (не участвовал в сравнении регрессий,
`docs/tasks/p2-test-track.md#test-3-срез-55-2026-09-22`). Триггер не найден — до и после
падения нет паники/зависшего теста в логе, симптом идентичен, но без диагностируемого
предшественника (в отличие от срезов выше, где `websockets`/`pointerevents`-триггеры были видны).
Два других `--check`-прогона `fetch` (до и после) прошли чисто на том же бинаре — разовая
нагрузочная флуктуация, не привязана к конкретному тесту категории.
