# BUG-794 — `element.focus()` never returns when called from inside a `load` event handler

**Статус:** OPEN
**Заведён:** 2026-08-20/21 (WPT-RUN-6, срез 2 — разбор массового TIMEOUT)
**Область:** `crates/js/src/dom.rs` (`HTMLElement.prototype.focus`, `_lumen_apply_ready_state`) и/или `crates/shell/src/main.rs` (`notify_window_loaded` / `route_task_js` вокруг перехода `readyState → 'complete'`, `main.rs:12486-12500`)
**Владелец:** P1/P3 (движок). Заведён P2 в ходе WPT-инструментальной задачи, здесь не чинится — корневая причина не локализована точнее «где-то в переходе на `complete`», нужен владелец с полным доступом к `crates/shell`+`crates/js`.

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

## Как проверить фикс

1. Репро выше: `window.__log` содержит `load-fired, focus-fired,
   focus-call-returned` (или хотя бы `focus-call-returned`) в течение
   разумного времени (не 25+ с).
2. `css/selectors/focus-visible-010.html` перестаёт быть TIMEOUT (переходит в
   harness OK).
3. Остальные `focus-visible-*` — часть должна перестать таймаутить; часть
   всё ещё может падать/таймаутить по независимой причине
   (`test_driver.send_keys` не реализован, см. выше) — не путать регрессию с
   уже известным лимитом.
