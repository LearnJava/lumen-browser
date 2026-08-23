# BUG-835 — `history.back()` через границу документа замораживает страницу: обхода нет, запроса нет, `pageshow` нет, и таймеры текущего документа больше не срабатывают

**Статус:** OPEN
**Заведён:** 2026-08-22 (WPT-RUN-6, срез 21 — найден живым замером, маркера намеренно нет)
**Область:** `crates/js/src/dom.rs:6941` (`history.go` → `_lumen_history_traverse`), `crates/shell/src/main.rs:20216`–`20329` (`navigate_back`: путь полного документа — bfcache-оттайка либо `reload()`)
**Владелец:** P1/P3 (`lumen-shell`). Заведён P2 в ходе WPT-задачи, здесь не чинится.

## Симптом

После настоящей (кросс-документной) навигации вызов `history.back()`
возвращает управление — и на этом всё заканчивается: предыдущий документ не
восстанавливается, URL не меняется, сетевого запроса нет, `pageshow` не
приходит, а **таймеры вызвавшего документа больше никогда не срабатывают**.
Для страницы это неотличимо от зависания.

## Прямое измерение

`tests/wpt/verify_navigation_form_import_gaps.py` (2026-08-22, dev-release,
Linux, коммит `762a0cad9`), обе пробы перезамерены с `--seconds 14`, чтобы
исключить «просто не успело»:

| проба | получено |
|---|---|
| `nav-back-wedges` | `script-start search=` → `script-start search=?second` → `tick 1` → `calling-back length=2` → `back-returned` — и тишина. Тиков за 14 с: **1** (ожидалось ~26), маркеров `t+500`/`t+2000` нет |
| `nav-back-cross-document` | `first-doc` → `pageshow persisted=false` → `second-doc` → `script-start search=?second` — и тишина. Тиков: **0** (`history.back()` вызывается на 300 мс, первый тик был бы на 500 мс) |

Сервер за это время не получил ни одного запроса, в stderr после
`back-returned` нет ни строки — ни ошибки, ни сообщения о загрузке.

Контроль, что дело именно в обходе, а не в навигации:
`--variant session-storage-across-reload` делает такую же кросс-документную
навигацию тем же способом и продолжает тикать до конца пробы (26 тиков).

## Причина (локализована частично)

JS-сторона доводит вызов до шелла: `history.back()` → `history.go(-1)`
(`dom.rs:6939`) → `_lumen_history_go(-1)` (зеркало-кэш) →
`_lumen_history_traverse(-1)`. Дальше `navigate_back` (`main.rs:20216`)
для записи полного документа либо оттаивает bfcache
(`BfCachePayload::Frozen` → `bfcache_thaw`, без сети), либо ставит
`self.reload()`. Наблюдаемо не происходит ни одного из двух: нет ни
запроса (значит не `reload()`), ни `pageshow persisted=true` (значит
оттайка не завершилась). Какая именно из половин теряет обход, замером со
стороны страницы не отделить — нужен инструментальный прогон со стороны
шелла, поэтому баг заведён на симптом с точной точкой входа.

## Масштаб

Маркера в `timeout_audit.py` намеренно нет: остаточные id
`html/browsers/history/the-history-interface/009|010`,
`back-pushstate-back-history-state.html`,
`overlapping-navigations-and-traversals/anchor-fragment-history-back-on-click.html`
гоняют обход через `<iframe>` или через клик по якорю и уже
атрибутированы более ранним причинам ([BUG-480](BUG-480-OPEN.md),
[BUG-833](BUG-833-FIXED.md)).

Отдельно стоит зафиксировать: заморозка касается **всего документа**, а не
только обхода. То есть один `history.back()` в шарде WPT — кандидат в
«зависший браузер» из механизма `hung-browser` (см. `timeout_audit.py`),
который забирает весь остаток шарда.

## Направление починки (не предписание)

Инструментировать `navigate_back` на пути полного документа (лог до/после
`bfcache.retrieve`, `bfcache_thaw`, `reload()`) и посмотреть, доходит ли
до него `_lumen_history_traverse(-1)` вообще: зеркало истории после
кросс-документной навигации — «read cache», и ветка `ok === false` в
`history.go` (`dom.rs:6952`) уходит перечитывать
`_lumen_navigation_entries_json()`, что при пустом зеркале может не дать
обхода, тихо и без ошибки.

## Как проверить фикс

1. `tests/wpt/.venv/bin/python tests/wpt/verify_navigation_form_import_gaps.py
   --variant nav-back-wedges --seconds 14` — ожидаются `t+500`/`t+2000` на
   первом документе и тики до конца пробы.
2. WPT: `run_report.py --all --root html/browsers/history --recursive`.
