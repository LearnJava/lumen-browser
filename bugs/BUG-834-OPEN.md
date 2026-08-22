# BUG-834 — при навигации уходящий документ не получает ни `unload`, ни `beforeunload`, ни `visibilitychange`; единственное, что приходит, — `pagehide` с `persisted=true`

**Статус:** OPEN
**Заведён:** 2026-08-22 (WPT-RUN-6, срез 21 — найден живым замером, маркера намеренно нет)
**Область:** `crates/shell/src/main.rs:20113` и `:20265` (единственные два места, откуда зовётся `fire_page_lifecycle("pagehide", …)`), `crates/js/src/dom.rs:7209` (`unload`-слушатели читаются только как блокировщик bfcache), `crates/js/src/dom.rs:13092` (`_lumen_apply_visibility` — зовётся с focus/blur окна, не с навигации)
**Владелец:** P1/P3 (`lumen-shell` + `lumen-js`). Заведён P2 в ходе WPT-задачи, здесь не чинится.

## Симптом

Страница, уходящая по `location.href = …`, слышит ровно одно событие
жизненного цикла:

```js
addEventListener("pagehide", e => …);        // приходит, e.persisted === true
window.onpagehide = …;                       // приходит
addEventListener("unload", …);               // НЕ приходит
addEventListener("beforeunload", …);         // НЕ приходит
addEventListener("visibilitychange", …);     // НЕ приходит
```

## Прямое измерение

`tests/wpt/verify_navigation_form_import_gaps.py --variant nav-pagehide-unload`
(2026-08-22, dev-release, Linux, коммит `762a0cad9`, `--seconds 6`;
навигация состоялась — сервер отдал `/pnfi-next.html?from=pagehide`,
новая страница жива и тикает):

| ожидалось | получено |
|---|---|
| `pagehide` + `unload` (+ `visibilitychange`), затем `next-page` | `navigating`, `pagehide persisted=true`, `onpagehide-prop`, `next-page search=?from=pagehide length=1` |

`persisted=true` — не описка замера: шелл кладёт уходящий документ в
bfcache и рапортует это флагом. Спека (HTML LS §7.4.6, шаг «unload a
document») допускает `persisted=true` только для документа, который
действительно сохраняется целиком; для обычной навигации по ссылке
браузеры дают `false`. Плюс `history.length` на новой странице остаётся
`1` — но это уже [BUG-829](BUG-829-OPEN.md)/история, не это.

## Причина (локализована чтением кода)

`fire_page_lifecycle` вызывается ровно из двух мест (`main.rs:20113`,
`:20265`) и только с литералом `"pagehide"`. Отправки `unload` в воркспейсе
нет вообще: единственное упоминание строки в шиме — `_lumen_bfcache_blocked`
(`dom.rs:7209`), где наличие `unload`-слушателя используется как *признак*
того, что страницу нельзя морозить. То есть про слушателей знают, а
доставку не делают. `beforeunload` — то же самое, плюс отсутствует
согласование отмены навигации. `_lumen_apply_visibility` (`dom.rs:13092`)
зовётся из Rust только на focus/blur окна, поэтому при уходе документа
`visibilityState` не переключается.

## Масштаб

Маркера в `timeout_audit.py` намеренно нет, и это осознанное решение: все
восемь id остатка, где ожидание стоит на `unload`/`pagehide`
(`html/browsers/browsing-the-web/unloading-documents/unload/006…009`,
`prompt/004`, `prompt-and-unload-script-closeable`,
`pagehide-on-history-forward`, `page-visibility/iframe-unload`), гоняют
навигацию **внутри `<iframe>`** и уже атрибутированы более ранней причине —
[BUG-480](BUG-480-OPEN.md): дочерний документ не запускается вовсе, так что
до вопроса об `unload` дело не доходит. Этот баг заведён по прямому
замеру и станет наблюдаемым в WPT, когда починят BUG-480.

## Направление починки (не предписание)

В обеих точках навигации (`main.rs:20106`, `:20261`) отправлять
последовательность из спеки: `beforeunload` (с учётом отмены) →
`visibilitychange`+`hidden` → `pagehide` → `unload`. Слушатели `unload`/
`beforeunload` уже собраны в `_other_win_listeners`, `_lumen_apply_visibility`
готов, добавить нужно только вызовы и корректный флаг `persisted`
(`true` — только когда документ действительно уходит в bfcache и
`_lumen_bfcache_blocked()` вернул `false`).

## Как проверить фикс

1. `tests/wpt/.venv/bin/python tests/wpt/verify_navigation_form_import_gaps.py
   --variant nav-pagehide-unload` — ожидаются `pagehide persisted=false`,
   `unload`, `visibilitychange state=hidden`.
2. WPT (после BUG-480): `run_report.py --all --root html/browsers/browsing-the-web/unloading-documents --recursive`.
