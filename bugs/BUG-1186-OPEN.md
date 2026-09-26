# BUG-1186 — `window.stop()` отсутствует

**Статус:** OPEN
**Заведён:** 2026-09-26 (P6, вынесен из [BUG-648](BUG-648-FIXED.md) при его закрытии; впервые
замечен P2 в WPT-VENDOR-performance-timeline, 2026-08-05).
**Область:** js (`crates/js/src/shim/web_api_shim_mid_b.js` — объект `window`; члена `stop` нет)
+ shell (отмена текущей навигации и загрузок документа).

## Симптом

`typeof window.stop` → `'undefined'`, вызов `window.stop()` → `TypeError: window.stop is not a
function`. WPT: `performance-timeline/not-restored-reasons/abort-block-bfcache.window.html`
падает именно на этом. Проба 2026-09-26, dev-release `--dump-layout`:
`<script>o.textContent = "STOP=" + typeof window.stop</script>` → `STOP=undefined`
(в Chrome — `function`).

## Что требуется

HTML LS, `Window.stop()`: метод, выполняющий «stop loading» для навигируемого документа
(отмена текущей навигации, прерывание fetch-загрузок документа, `document.readyState` не
переходит дальше). Минимальный срез — сам метод на `Window.prototype`: страница, которая его
вызывает, перестаёт падать с `TypeError`, а навигация, ещё не зафиксированная shell-ом,
отменяется. Критерий: `typeof window.stop === 'function'`, и тест WPT выше доходит до своих
утверждений.
