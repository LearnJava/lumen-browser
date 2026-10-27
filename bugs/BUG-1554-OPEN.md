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

## Срез 1 (2026-10-28, P3) — влит

Сделано: единый вердикт «лист применяется» `Document::sheet_owner_enabled` (`crates/engine/dom/src/sheet_sets.rs`) для каскада шелла и реестра JS; предпочитаемый набор (`title`/`alternate`/`<meta http-equiv=default-style>`, набор — по первому **созданному** листу с заголовком, не по порядку в дереве); `HTMLLinkElement.disabled` (атрибут + «явно включён»), `CSSStyleSheet.disabled`/`HTMLStyleElement.disabled` для листов, принадлежащих элементу; `<link disabled>` — не лист и не в `document.styleSheets`; тело `<link>`, пришедшее после каскада шелла, входит в реестр и каскад (`_lumen_stylesheet_register_link`), `data:`-лист грузится без запроса; клон `<link>` грузится при вставке; `load`/`error` не стреляют на вынутом из дерева `<link>`; headless-снимок после `load` накладывает `patch_cascade` (`dump_mode::settle_after_load`). Подробности — `subsystems/js.md` §Done.

WPT `css/cssom` (225 файлов): `HTMLLinkElement-disabled-001…004,006`, `HTMLLinkElement-load-event-002`, `preferred-stylesheet-order`, `stylesheet-title` — PASS (были красными, их `.ini` удалены); `HTMLLinkElement-disabled-alternate` (reftest) остаётся PASS уже за правильную причину. `html/semantics/document-metadata`: отклонений от прогона на бинаре до правки нет, `the-style-element/style_disabled.html` стал зелёным.

Остаток:

- `link-element-stylesheet-title`, `ttwf-cssom-doc-ext-load-count`, `ttwf-cssom-doc-ext-load-tree-order` ждут лист и у `<link>`, чей файл не пришёл (404): браузеры оставляют его в `document.styleSheets`, у нас не-загрузившийся `<link>` листа не имеет.
- Смена `rel`/`href`/`title` через `setAttribute` не перезагружает лист (нет общего хука на смену атрибута `<link>`); почему `HTMLLinkElement-disabled-005` проходит — не выяснено.
- `HTMLLinkElement-load-event` («не блокирует парсер»), `document.selectedStyleSheetSet`/`lastStyleSheetSet`.
