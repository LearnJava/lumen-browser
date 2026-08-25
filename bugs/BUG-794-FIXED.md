# BUG-794 — `element.focus()` never returns when called from inside a `load` event handler

**Статус:** FIXED 2026-08-25 (P1)
**Заведён:** 2026-08-20/21 (WPT-RUN-6, срез 2 — разбор массового TIMEOUT)
**Область (по факту):** `crates/js/src/v8_runtime.rs` — `named_access_lookup` / новый `lock_document_bounded`, интерцептор именованных свойств окна (HTML LS §7.3.3, BUG-384). Заявленная область (`dom.rs::HTMLElement.prototype.focus`, `_lumen_apply_ready_state`, `main.rs::notify_window_loaded`) оказалась ни при чём.
**Владелец:** P1 (движок). Заведён P2 в ходе WPT-инструментальной задачи.

---

## Что оказалось на самом деле (замер P1, 2026-08-25)

**`focus()` возвращается — и возвращался всегда.** Проба
`tests/wpt/verify_bug794_focus_in_load.py` (тот же харнесс, что у срезов
15/17–22/24: один процесс браузера на страницу, http, доказательства со
stderr браузера, сердцебиение `setInterval` до `load`) печатает у варианта
`load-focus` весь ряд маркеров: `load-fired, focus-fired, load-returned
active=el, load-handler-end`. Ни одна из шести контрольных форм (без
`preventScroll`, из `DOMContentLoaded`, из таймера, другой натив из того же
обработчика, обработчик вообще без нативов) не зависает.

**Заявочная проба спотыкалась на другом.** Она берёт элемент через
*именованный доступ окна* (`el`, без `var` — HTML LS §7.3.3), и вот это
внутри обработчика `load` — и **только** там — даёт
`Uncaught ReferenceError: el is not defined`. Первым падает не `focus()`, а
`el.addEventListener('focus', …)` строкой выше, поэтому `__log` навсегда
остаётся `["load-fired"]`. До [BUG-591](BUG-591-FIXED.md) (2026-08-23) это
исключение глотал голый `catch (e) {}` в `_lumen_apply_ready_state` — ни
события `error`, ни строки на stderr, ни попадания в `try/catch` страницы,
то есть ровно та картина «вызов не вернулся», которую описывает заявка.
Воспроизведение заявки дословно (MCP-опрос `window.__log`) — `.tmp`-скрипт
на 60 строк; после правки тот же опрос даёт
`["load-fired","focus-fired","focus-call-returned"]`.

**Механизм — гонка потоков, а не фаза жизненного цикла.** Вариант
`named-access` спрашивает `typeof el` / `'el' in window` /
`document.getElementById('el')` в пяти фазах:

| фаза | до правки |
|---|---|
| верхний уровень скрипта | `object`, `true`, ok |
| `DOMContentLoaded` | `object`, `true`, ok |
| `window` `load` | **`undefined`, `false`**, ok |
| `requestAnimationFrame` | `object`, `true`, ok |
| таймер | `object`, `true`, ok |

Инструментированная сборка показала причину: `named_access_lookup` брал
документ через `try_lock` (сознательно — интерцептор срабатывает на **любой**
промах глобального имени, в том числе из JS, вызванного нативом под той же
блокировкой, где блокирующий `lock()` — самодедлок), а диспатч `load` с
2026-08 идёт через **движковый поток** (ADR-023, `route_task_js`), то есть
исполняется *параллельно* с проходом UI-потока по документу. Замер: блокировку
держит другой поток **3.9 мс**, `try_lock` этот забег проигрывает и
интерцептор отказывается отвечать. Соседние `document.getElementById`
работают в тот же момент именно потому, что их натив ждёт блокировку
(`lock()`), а не сдаётся. Контроль: с `LUMEN_NO_ENGINE_THREAD=1`
(синхронный вызов с UI-потока) дефекта нет ни в одной фазе.

## Правка

`lock_document_bounded` (`crates/js/src/v8_runtime.rs`): ограниченное ожидание
блокировки — `try_lock` в цикле, шаг 100 мкс, бюджет 20 мс, дальше отказ
(прежнее поведение). Щедро против замеренных 3.9 мс и по-прежнему **ограничено**,
поэтому случай, ради которого стоял `try_lock` — блокировка уже у этого же
потока — стоит задержки, а не дедлока. Три юнит-теста: свободная блокировка
берётся сразу; удержание другим потоком (4 мс) выжидается; блокировка, которую
не отпустят, отдаёт `None` не раньше бюджета.

## A/B на WPT

9 id `css/selectors/focus-visible-*`, не использующих `test_driver`
(остальные 42 упираются в `getClientRects`, [BUG-478](BUG-478-OPEN.md) —
см. `CLAUDE.md`), `run_smoke.py`, тот же бинарник до и после:

* `focus-visible-010.html`: **ERROR → OK** (harness);
* остальные 8 — без изменений;
* `focus-visible-020.html` даёт `UNEXPECTED-OK` **и до, и после** правки —
  устаревший `.ini` от WPT-RUN-3, к этой правке отношения не имеет
  (кандидат на `--update-expected` владельцу дорожки).

## Остаток

Сабтест `focus-visible-010` («Programmatic focus on page load should match
`:focus-visible`») по-прежнему FAIL, как и записано в его `.ini`: в момент
обработчика `focus` вычисленный `outline-color` — `auto`, стиль `:focus`
шелл применяет только на следующем прогоне ([BUG-560](BUG-560-OPEN.md)).
Это отдельный дефект, не этот.

---

## Исходная заявка (как была подана)

## Симптом

`element.focus()`, вызванный **синхронно** изнутри обработчика `window`
`load`-события (`window.addEventListener('load', ...)` или `window.onload`),
**никогда не возвращает управление** — ни `resolve`, ни исключение (даже
необрабатываемое V8-исключение, которое ловится `try/catch`), скрипт просто
останавливается на этой строке. Проверено удержанием опроса **25+ секунд**
(намного дольше любого разумного WPT-таймаута) — это не «медленно», а именно
зависание.

## Минимальное репро (без test_driver, без WPT — чистый JS в живом окне)

```html
<div id="el" tabindex="-1">hi</div>
<script>
window.__log = [];
window.addEventListener('load', () => {
  window.__log.push('load-fired');
  el.addEventListener('focus', () => window.__log.push('focus-fired'));
  try {
    el.focus({preventScroll: true});   // preventScroll rules out scrollIntoView() as the cause
    window.__log.push('focus-call-returned');
  } catch (e) {
    window.__log.push('focus-call-threw: ' + e.message);
  }
});
</script>
```

Проверено через `--mcp-live-port` (`tools/call eval`): `window.__log` после
25 с — **`["load-fired"]`**, ни `focus-fired`, ни `focus-call-returned`, ни
`focus-call-threw` не появляются никогда.

## Изоляция — воспроизводится только на переходе `readyState → 'complete'`

Тот же паттерн (регистрация слушателя `focus` + `el.focus()`), проверенный в
трёх других контекстах на той же странице — работает мгновенно и корректно:

| контекст вызова | результат |
|---|---|
| `window.addEventListener('load', …)` | **зависает** (это репро) |
| `document.addEventListener('DOMContentLoaded', …)` (переход на `'interactive'`) | OK, `focus-fired` приходит сразу |
| `window.addEventListener('zzz', …)` + `window.dispatchEvent(new Event('zzz'))` (чисто JS-диспетчеризация, без нативного вызова) | OK, `focus-fired` приходит сразу |
| прямой вызов `el.focus()` через `eval` вне какого-либо обработчика | OK |

То есть баг не в `.focus()` вообще (он явно работает) и не в «любом
нативно-инициированном вызове JS» (та же машинерия отрабатывает на
`DOMContentLoaded`) — он специфичен именно моменту, когда
`crates/shell/src/main.rs:12497-12500` диспетчерует
`js.notify_window_loaded()` (→ `_lumen_apply_ready_state('complete')` →
слушатели `load`). `{preventScroll: true}` исключает безусловный
`scrollIntoView()` внутри `HTMLElement.prototype.focus` (`dom.rs:13125-13135`)
как причину.

**Не локализовано глубже:** какая именно операция внутри цепочки
`_lumen_request_focus` → `_lumen_focus_update` → `_lumen_dispatch_focus_event`
(или что-то в самом переходе `notify_window_loaded`/`route_task_js`/движковый
поток ADR-016 M2.2) блокируется — нужен трейс с точечными логами/брейкпоинтом
внутри `crates/shell` у того, кто чинит.

## Масштаб

**Прямое подтверждение** (без testdriver, чистый механизм): семейство
`css/selectors/focus-visible-*.html` — 25 из 52 запущенных id TIMEOUT в
корпусном снимке WPT-RUN-5 (Windows, `docs/wpt/runs/2026-08-20-windows-partial.json`),
из них `focus-visible-010.html` — репро выше в чистом виде (тест вообще не
использует `test_driver`, только `window.addEventListener('load', …)` +
`el.focus()`). Часть остальных id того же семейства (007/012/013 и т.п.)
дополнительно используют `test_driver.click()`/`send_keys()` — там на
результат может влиять ещё и известный пробел `send_keys`/прочих действий,
не реализованных `executorlumen.py` (см. его собственный докстринг,
«every other action fails cleanly… — the DoD is "not silently SKIPped"»,
`tools/wptrunner/wptrunner/executors/executorlumen.py:34-52»); это отдельный,
уже задокументированный лимит инструментария, не путать с данным движковым
багом.

**Не измерено точно, но грубая оценка охвата** (правило «no silent caps» —
явно помечаю как неточную): `grep` по вендоренному корпусу `tests/wpt/` на
совместное присутствие `addEventListener('load'`/`window.onload` и `.focus(`
в одном файле даёт **162 файла** — верхняя граница потенциально затронутых
(часть вызывает `.focus()` не из самого `load`-обработчика, а из вложенной
функции/таймера, и такие останутся не затронуты этим багом; часть — уже
покрыта другими причинами TIMEOUT). Требует индивидуальной проверки, не
принимать как точную цифру.

**Значение вне WPT.** Паттерн «сфокусировать элемент после полной загрузки
страницы» (автофокус первого невалидного поля формы, фокус-ловушки,
доступность) — обычная практика на реальных сайтах, а не только в тестах;
это движковый баг с потенциальным влиянием на реальный браузинг, не только
на очки WPT.

## Как проверить фикс (критерии заявки — оба выполнены)

1. Репро выше: `window.__log` содержит `load-fired, focus-fired,
   focus-call-returned` (или хотя бы `focus-call-returned`) в течение
   разумного времени (не 25+ с). ✅ 2026-08-25.
2. `css/selectors/focus-visible-010.html` перестаёт быть TIMEOUT (переходит в
   harness OK). ✅ 2026-08-25 (к моменту правки он был уже не TIMEOUT, а
   ERROR — BUG-591 довёл проглоченное исключение до харнесса).
3. Остальные `focus-visible-*` — часть должна перестать таймаутить; часть
   всё ещё может падать/таймаутить по независимой причине
   (`test_driver.send_keys` не реализован, см. выше) — не путать регрессию с
   уже известным лимитом.
