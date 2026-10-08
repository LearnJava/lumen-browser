# BUG-1554 — `link.disabled` и `<style>.disabled` не работают, `document.styleSheets` под `--mcp` без таблиц из `<link>`, альтернативные таблицы по `title` не переключаются

**Статус:** OPEN
**Заведён:** 2026-10-08 (P2, WPT-RUN-14 срез 25, `css/css-typed-om` + `cssom-view` + `cssom` + `css-lists` + `css-counter-styles`)
**Область:** js/shell (`crates/js/src/shim/web_api_shim_mid.js` — `HTMLLinkElement.disabled`/`sheet`; `crates/shell/src/stylesheets.rs`)

## Симптом

`document.createElement("link").disabled` — `undefined` (атрибут отражается не в IDL-свойство); `style.disabled = true` отражается на элементе, но `style.sheet.disabled` остаётся `false`; `document.styleSheets` под `--mcp` с `<link rel=stylesheet href=ext.css>` и `<link rel="alternate stylesheet" title=alt>` — длина 0. Тесты `HTMLLinkElement-disabled-*`, `link-element-stylesheet-title`, `stylesheet-title`, `preferred-stylesheet-order`, `ttwf-cssom-doc-ext-load-*` (22 id, 52 из 63 сабтестов). Проба шла под `--mcp`; поведение в окне (shell загружает `<link>` отдельно) не проверялось.

## Проба

probe (`--mcp`):

| вызов | у нас | ожидается |
|---|---|---|
| `document.createElement("link").disabled` | `undefined` | `false` |
| `<style title=x>` → `style.disabled=true` → `style.sheet.disabled` | `false` | `true` |
| `document.styleSheets.length` при `<link rel=stylesheet href=ext.css>` + `<link rel="alternate stylesheet" title=alt href=ext.css>` | 0 | 2 |

## Как найдено

WPT-RUN-14 срез 25: `css/cssom/{HTMLLinkElement-disabled-001,002,003,004,006,HTMLLinkElement-load-event,HTMLLinkElement-load-event-002,link-element-stylesheet-title,stylesheet-title,preferred-stylesheet-order,ttwf-cssom-doc-ext-load-count,ttwf-cssom-doc-ext-load-tree-order,CSSStyleSheet,style-sheet-interfaces-001,stylesheet-same-origin.sub,MutationObserver-style,insertRule-across-context,insertRule-from-script,insertRule-syntax-error-01,selectorText-modification-restyle-*}.html`.

## Что делать

`HTMLLinkElement.disabled` как IDL-свойство и отражение в `sheet.disabled`; таблицы `<link>` и `<style>` в `document.styleSheets` в порядке дерева; выбор по `title` (HTML LS §4.2.4 «alternative style sheet sets»).

## Как проверить

`css/cssom/HTMLLinkElement-disabled-001.html`, `stylesheet-title.html`.
