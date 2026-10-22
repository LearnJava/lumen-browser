# BUG-1676 — microsoft.com / cnn.com не доходят до `DocumentReady` за 90 с, bbc.com — «слишком долго не отвечает»

**Статус:** OPEN
**Заведён:** 2026-10-21 (P2, TEST-11 — [geom-diff-2026-10-21](../docs/quality/geom-diff-2026-10-21.md))
**Область:** не локализовано (сеть / загрузка подресурсов / JS главного потока)

## Симптом

Прогон `scripts/geom_diff.py` (Lumen — сборка `target/dev-release` от 2026-10-21 23:02, `LUMEN_NO_ADBLOCK=1`,
окно 1280×720, `--nav-timeout 60`):

| сайт | Lumen | Chrome |
|---|---|---|
| `https://www.microsoft.com/` | `page still loading` после ~90 с (readyState не `complete`) | 9–13 с |
| `https://www.cnn.com/` | то же | 9–13 с |
| `https://www.bbc.com/` | окно показывает страницу «слишком долго не отвечает» (ошибка сети Lumen) | 9–13 с |
| `https://ria.ru/` | `script.evaluate` голодает за redraw-задачами: первый снимок не снялся, второй — за 118 с | быстро |

Три из этих сайтов не попадают в сравнение вообще, поэтому о геометрии на них ничего не известно.

## Что делать

Локализовать на каждом сайте отдельно: какие запросы висят (`network.responseCompleted` не приходит —
журнал `LUMEN_LOG=net`), какой ресурс держит `readyState` в `loading`, не крутится ли главный поток
в JS (`scripts/perf_audit.py` покажет long tasks). Для ria — почему очередь команд BiDi голодает за
перерисовками (`engine thread busy`). Затем завести BUG на каждую первопричину.

## Как проверить

`python scripts/geom_diff.py --only microsoft cnn bbc ria` — все четыре снимаются в пределах
`--nav-timeout` и попадают в таблицу сравнения.
