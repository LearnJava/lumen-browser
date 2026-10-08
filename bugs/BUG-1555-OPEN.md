# BUG-1555 — `adoptedStyleSheets`: стили не применяются в shadow root, `push` бросает `TypeError`, `adoptedStyleSheets !== adoptedStyleSheets`

**Статус:** OPEN
**Заведён:** 2026-10-08 (P2, WPT-RUN-14 срез 25, `css/css-typed-om` + `cssom-view` + `cssom` + `css-lists` + `css-counter-styles`)
**Область:** js/layout (`crates/js/src/shim/web_api_shim_mid.js` — `adoptedStyleSheets`; применение в каскаде `crates/shell/src/page_pipeline.rs::build_page_cascade`)

## Симптом

`document.adoptedStyleSheets = [s]` применяется (`#t{background:red}` — красный на снимке); в shadow root — нет: `sr.adoptedStyleSheets = [s]` с `p{background:blue}` — пиксель белый, а `<style>` внутри того же shadow root применяется (зелёный). `document.adoptedStyleSheets.push(s)` — `TypeError: Cannot add property 1, object is not extensible` (по спецификации — `ObservableArray`, `push` допустим). `document.adoptedStyleSheets === document.adoptedStyleSheets` — `false`, `new CSSStyleSheet().cssRules === cssRules` — `false`. 21 id (`CSSStyleSheet-constructable*`, `adoptedstylesheets-*`, `StyleSheetList-constructable*`).

## Проба

probe (`--mcp`):

| вызов | у нас | ожидается |
|---|---|---|
| screenshot: `shadowRoot.adoptedStyleSheets=[s]`, `s=p{background:blue}`; пиксель внутри `<p>` | белый | синий |
| screenshot: `<style>p{background:green}</style>` в shadow root | зелёный | зелёный |
| `document.adoptedStyleSheets.push(s)` | `TypeError: object is not extensible` | длина 1 |
| `document.adoptedStyleSheets === document.adoptedStyleSheets` | `false` | `true` |

## Как найдено

WPT-RUN-14 срез 25: `css/cssom/{CSSStyleSheet-constructable*,adoptedstylesheets-*,StyleSheetList-constructable*,CSSStyleSheet-template-adoption}.html` (21 id).

## Что делать

Применять `adoptedStyleSheets` shadow root (и `ShadowRoot`-фрагмента) в каскаде наравне с `<style>`; сделать массив `ObservableArray` с идентичностью и мутирующими методами (`push`/`splice`); стабильный `cssRules` у `CSSStyleSheet`.

## Как проверить

`css/cssom/adoptedstylesheets-observablearray.html`, `StyleSheetList-constructable-shadow.html`.
