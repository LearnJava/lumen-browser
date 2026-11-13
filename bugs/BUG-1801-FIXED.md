# BUG-1801 — нет `window.status`: bare `status` в скрипте ebay.com даёт uncaught `ReferenceError: status is not defined`

**Статус:** FIXED
**Открыт:** 2026-11-12 (P3, перф-аудит ebay)
**Исправлен:** 2026-11-13 (P3)
**Компонент:** js (`crates/js/src/shim/web_api_shim_tail_b.js` — секция глобалей окна; в карточке при заведении указывался `web_api_shim_mid.js` — там своих геттеров `status` действительно два, FontFace и документ, но глобали окна живут в `tail_b`)

## Симптом

`https://www.ebay.com/`, dev-release, живое окно, `LUMEN_NO_ADBLOCK=1` (3 compat-прогона 2026-11-12): во всех трёх в stderr

```
[JS error] Uncaught ReferenceError: status is not defined
```

Скрипт ebay использует bare-идентификатор `status`; в браузерах он резолвится в `window.status` (HTML-спека: атрибут `Window.status`, строка с геттером/сеттером). В шиме Lumen `window.status` не определён (grep по `web_api_shim_mid.js`: единственные `status:`-геттеры — у FontFace и документа), поэтому идентификатор не находится и скрипт падает uncaught.

## Гипотеза

Реализовать `window.status` по спецификации: собственный строковый геттер/сеттер (сеттер — no-op с сохранением значения, как в браузерах), definable на глобальном окна. После этого bare `status` резолвится.

## Исправление

- `Object.defineProperty(window, 'status', …)` с замыканием-хранилищем в IIFE — сразу после блока
  `origin` (BUG-1208) в `web_api_shim_tail_b.js` (файл исполняется в page-контексте после
  `window = globalThis`, поэтому accessor попадает и на глобал — bare `status` резолвится).
  Геттер возвращает сохранённое значение (начальное `''`), сеттер — `String(v)` (WebIDL DOMString —
  ToString-коэрция); видимого статус-бара в современных браузерах нет, значение только хранится.
  `enumerable`/`configurable: true` — обычная форма WebIDL-атрибута.
- Воркеров не касается: `status` — атрибут `Window`, не `WorkerGlobalScope`.
- Регрессионный тест: `crates/js/tests/cases/bug1801_window_status.rs` (3 теста — начальное
  значение и резолв bare-идентификатора, roundtrip через bare и `window.status`,
  DOMString-коэрция `123`→`"123"`, `null`→`"null"`).

## Репро

`lumen --maximized "https://www.ebay.com/"` и смотреть stderr, либо `eval` скрипта `status` в живом окне — ReferenceError.

Отчёт: [docs/perf/runs/2026-11-12-ebay-audit.md](../docs/perf/runs/2026-11-12-ebay-audit.md) §3.1. Соседний пробел того же класса — BUG-1782 (методы `console`).
