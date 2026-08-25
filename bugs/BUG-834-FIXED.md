# BUG-834 — при навигации уходящий документ не получает ни `unload`, ни `beforeunload`, ни `visibilitychange`; единственное, что приходит, — `pagehide` с `persisted=true`

**Статус:** FIXED 2026-08-25
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
`1` — но это уже [BUG-829](BUG-829-FIXED.md)/история, не это.

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

## Перезамер (WPT-RUN-6, срез 28, 2026-08-23)

`verify_window_history_jsurl_gaps.py --variant unload-nav` (обычная
навигация `location.href = …`) и `--variant win-close` (`window.close()`):
`pagehide` приходит на навигации и не приходит на `close()`; `beforeunload`
и `unload` не приходят ни там, ни там. То есть у этого бага две разные
половины — событие, которого нет в принципе (`beforeunload`/`unload`), и
путь выгрузки, который `close()` не запускает вовсе
([BUG-887](BUG-887-OPEN.md)).

## Починка (P1, 2026-08-25)

Заявка описывала один дефект — «нет отправки». Их оказалось **два**, и
второй объясняет, почему в замере стоял `persisted=true`, хотя обычная
навигация по ссылке ничего не сохраняет.

**1. Последовательность выгрузки.** В шиме
(`crates/js/src/dom.rs`) появился `_lumen_unload_document(persisted)` —
шаги HTML LS §7.4.6 в спековом порядке: `pagehide` → `visibilityState =
'hidden'` → `unload`, — и `_lumen_fire_beforeunload()` — «prompt to unload a
document» §7.4.5. Гейтом служит флаг `_lumen_page_showing` («page showing»
из спеки): он делает последовательность идемпотентной, а `pageshow`
поднимает его обратно вместе с `_lumen_apply_visibility(false)` — иначе
страница, вернувшаяся из bfcache в **том же** рантайме (парковка
[BUG-835](BUG-835-FIXED.md)), осталась бы навсегда `hidden` и второго
`pagehide` уже не получила бы. `unload` идёт через
`window.dispatchEvent`: у него нет соглашения о возвращаемом значении
обработчика, а общая ветка `dispatchEvent` уже даёт нужный порядок
(слушатели, затем `window.onunload`) с `_lumen_report_exception` на каждом.
`beforeunload` — отдельный цикл именно из-за этого соглашения: строку,
возвращённую из `window.onbeforeunload`, спека требует положить в
`returnValue`, а возврат из `addEventListener`-слушателя — нет. Шелл зовёт
обе функции из трёх точек навигации (`navigate_to`, `navigate_back`,
`navigate_forward`) через новые `PersistentJs::unload_document` /
`fire_beforeunload`.

**2. `persisted` поднимала ветка, которая ничего не сохраняет.** В
`navigate_to` последним шагом стоит fallback «сохранить HTML-снапшот, если
заморозка не удалась», и он ставил `persisted = true` **не проверяя**
`bfcache_eligible`. Но снапшот — это исходный текст страницы: возврат
парсит документ заново, ни слушателей, ни таймеров, ни замыканий не
остаётся. По §7.4.6 это discarded, а не salvageable. Присваивание убрано;
salvageable теперь ровно две ветки — парковка целиком и заморозка DOM.
Это же и решает, стрелять ли `unload`: спека шлёт его **только**
несохранённому документу, а страница со слушателем `unload`/`beforeunload`
и так не проходит `_lumen_bfcache_blocked()`, так что обе половины
сходятся. В `navigate_back`/`navigate_forward` признак вычисляется до
последовательности (`outgoing_parkable`) и переиспользуется веткой
парковки — иначе флаг, сообщённый странице, мог бы разойтись с тем, что
шелл сделал через десяток строк.

**Отмена навигации из `beforeunload` намеренно не сделана.** Ответ
страницы («прошу остаться») вычисляется и возвращается, но не исполняется:
для него нужен пользовательский диалог, а `confirm()` в этом движке —
заглушка, всегда возвращающая `false`. Считать «прошу остаться» за
«отменить» значило бы намертво заклинить любую страницу, выставляющую
`returnValue`, без способа сказать «уходим».

### Замер после фикса

`verify_navigation_form_import_gaps.py --variant nav-pagehide-unload`
(2026-08-25, Windows, dev-release, `--seconds 6`):

| до | после |
|---|---|
| `navigating`, `pagehide persisted=true`, `onpagehide-prop`, `next-page` (2 тика) | `navigating`, **`beforeunload`**, `pagehide persisted=false`, `onpagehide-prop`, **`visibilitychange state=hidden`**, **`unload`**, `next-page search=?from=pagehide length=1` (8 тиков) |

A/B на одной машине против сборки того же коммита без правки: пять
контрольных вариантов (`control`, `nav-back-cross-document`,
`nav-back-wedges`, `session-storage-across-reload`, `nav-location-reload`)
дают побайтово тот же список маркеров — парковка, оттайка и
`sessionStorage` не задеты. Полный прогон пробы (28 вариантов) расхождений
с задокументированным поведением не показал.

Юнит-тесты: 7 штук в `dom.rs` (порядок трёх событий, `persisted=true` не
даёт `unload`, `onunload`, идемпотентность флага, восстановление флага на
`pageshow`, `preventDefault`/`returnValue` у `beforeunload`, `'onunload' in
window`).

### Не входило

- `location.reload()` и `window.close()` последовательность выгрузки не
  запускают вовсе — `reload()` зовут изнутри и сами пути навигации, так
  что отправка оттуда дала бы двойное событие; `close()` — это
  [BUG-887](BUG-887-OPEN.md).
- Отмена навигации по `beforeunload` (нужен диалог, см. выше).
- `unload` не несёт «legacy target override flag» — `event.target` пуст,
  как и у всех оконных событий ([BUG-873](BUG-873-OPEN.md)).
- WPT-наблюдаемость по-прежнему за [BUG-480](BUG-480-OPEN.md): все восемь
  остаточных id гоняют навигацию внутри `<iframe>`.
