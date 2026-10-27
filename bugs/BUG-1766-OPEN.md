# BUG-1766 — `<link rel=icon>` шлёт `error` при неудачной загрузке

**Статус:** OPEN
**Заведён:** 2026-10-27 (P2, WPT-HOST-WPT — тест был ложным PASS на хосте `localhost`)
**Область:** js shim (`crates/js/src/shim/web_api_shim_mid.js` — `_lumen_link_hint_kind`, `_lumen_link_hint_prepare`, `_lumen_link_hint_fetch`, `_lumen_link_hint_failed`, `_lumen_link_hint_settle`)

## Симптом

`html/links/icon/no-error-event.sub.html`: `<link rel=icon href="http://nonexistent.{{host}}/…">` — тест ждёт две секунды и падает, если на элементе пришло `error`. В Lumen приходит `error`.

На старом хосте (`localhost`) тест проходил случайно: ошибка DNS приходила позже двухсекундного окна. С `LUMEN_RESOLVE` `nonexistent.*` → `~NOTFOUND` отказ мгновенный, и `error` успевает.

HTML LS §4.6.7.9 (link type `icon`): иконки — ресурс, который UA получает по своему усмотрению; для `icon` не определены ни шаги «process the linked resource», ни события `load`/`error`. Chrome и Firefox не шлют на `<link rel=icon>` ни `load`, ни `error`.

## Причина

`_lumen_link_hint_kind` ([web_api_shim_mid.js:16013](../crates/js/src/shim/web_api_shim_mid.js#L16013)) возвращает `'icon'` наравне с `preload`/`prefetch` (BUG-848), и `_lumen_link_hint_prepare` (:16183) ведёт иконку по общему пути подсказок: `_lumen_link_hint_fetch` (:16064) → `_lumen_link_hint_failed` / `_lumen_link_hint_settle` → `_lumen_resource_fire(nid, 'error')` (и `load` при успехе).

## Что делать

1. Для `kind === 'icon'` загрузку (если она нужна для фавикона) вести без `_lumen_resource_fire`: ни `load`, ни `error`.
2. Проверить, что BUG-848 (ради чего `icon` попал в список) не требует события `load` — если требует, сверить с WPT `html/links/icon/*` и поведением Chrome.

## Как проверить

- `run_smoke.py` на `/html/links/icon/no-error-event.sub.html` — PASS при `browser_host: web-platform.test`.
- Остальные `html/links/icon/*` и `html/semantics/document-metadata/the-link-element/*` — без регрессий.
