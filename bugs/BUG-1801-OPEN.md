# BUG-1801 — нет `window.status`: bare `status` в скрипте ebay.com даёт uncaught `ReferenceError: status is not defined`

**Статус:** OPEN
**Открыт:** 2026-11-12 (P3, перф-аудит ebay)
**Компонент:** js (`crates/js/src/shim/web_api_shim_mid.js` — глобальные окна)

## Симптом

`https://www.ebay.com/`, dev-release, живое окно, `LUMEN_NO_ADBLOCK=1` (3 compat-прогона 2026-11-12): во всех трёх в stderr

```
[JS error] Uncaught ReferenceError: status is not defined
```

Скрипт ebay использует bare-идентификатор `status`; в браузерах он резолвится в `window.status` (HTML-спека: атрибут `Window.status`, строка с геттером/сеттером). В шиме Lumen `window.status` не определён (grep по `web_api_shim_mid.js`: единственные `status:`-геттеры — у FontFace и документа), поэтому идентификатор не находится и скрипт падает uncaught.

## Гипотеза

Реализовать `window.status` по спецификации: собственный строковый геттер/сеттер (сеттер — no-op с сохранением значения, как в браузерах), definable на глобальном окна. После этого bare `status` резолвится.

## Репро

`lumen --maximized "https://www.ebay.com/"` и смотреть stderr, либо `eval` скрипта `status` в живом окне — ReferenceError.

Отчёт: [docs/perf/runs/2026-11-12-ebay-audit.md](../docs/perf/runs/2026-11-12-ebay-audit.md) §3.1. Соседний пробел того же класса — BUG-1782 (методы `console`).
