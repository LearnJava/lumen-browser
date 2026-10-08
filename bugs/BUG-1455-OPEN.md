# BUG-1455 — CSSOM не знает at-правил кроме `@media`: `insertRule('@layer …')` бросает `SyntaxError`, `cssRules` пропускает `@layer`/`@scope`/`@supports`/`@container`/`@font-face`/`@keyframes`/`@property`, вложенные правила развёрнуты в плоские

**Статус:** OPEN (ДОРАБОТКА → CSSOM-10)
**Заведён:** 2026-10-08 (P2, WPT-RUN-14 срез 20, `css/selectors` + `css-pseudo` + `css-nesting` + `css-namespaces` + `css-cascade`)
**Область:** js/css-parser (CSSOM правил: `CSSStyleSheet.insertRule`/`cssRules` для at-правил, `CSSGroupingRule`/`CSSLayerBlockRule`/`CSSScopeRule`/`CSSNestedDeclarations`, `cssRules` у `CSSStyleRule`)

## Симптом

(1) `sheet.insertRule(text, index)` принимает только обычное правило и `@media`: `@layer first, second`, `@layer a{…}`, `@import`, `@scope`, `@supports`, `@container`, `@font-face`, `@keyframes`, `@property`, `@namespace`, `@page` бросают `SyntaxError: … the supplied text is not a valid rule`. (2) Для листа, состоящего из одного из перечисленных at-правил, `cssRules.length` = 0; в смешанном листе `cssRules` отдаёт только `CSSStyleRule` и `CSSMediaRule`. (3) Вложенные правила разворачиваются при разборе в плоские: `#y{color:red; & b{color:blue}}` → два верхнеуровневых `CSSStyleRule` (`#y`, `#y b`), у первого `cssRules` = `undefined`. (4) Глобалов `CSSGroupingRule`, `CSSLayerBlockRule`, `CSSLayerStatementRule`, `CSSScopeRule`, `CSSNestedDeclarations` нет (`typeof` = `undefined`). Это 18 id в одном кластере (`layer-cssom-order-reverse`, `scope-cssom`, `all-prop-revert-layer`, `idlharness`, `nested-declarations-cssom*`, `serialize-group-rules-with-decls`, `cssom.html`, `layer-rules-cssom` и др.: 203 из 225 сабтестов) В тех же и ещё нескольких id — 46 сабтестов с `the supplied text is not a valid rule` (7 id: `parsing/layer-import-parsing`, `supports-import-parsing`, `at-scope-relative-syntax`, `layer-statement-before-import`, `layer-cssom-order-reverse*`, `attribute-case/cssom`). Относится к семейству CSSOM-8 (запись для собственных листов): [BUG-1220](BUG-1220-OPEN.md) — тот же `SyntaxError` на странице.

## Проба

`<style id=st>a{}</style>`, `sheet = st.sheet`, `sheet.insertRule(⟨текст⟩, 0)`:

| текст | результат | ожидается |
|---|---|---|
| `b{}` | `2→3` правил | верно |
| `@media print{b{}}` | `1→2` | верно |
| `@layer first, second`, `@layer a{b{}}` | **SyntaxError** | вставлено |
| `@import url(x.css)`, `@import url(x.css) layer(a)` | **SyntaxError** | вставлено |
| `@scope (a){b{}}`, `@supports (color:red){b{}}`, `@container (min-width:1px){b{}}` | **SyntaxError** | вставлено |
| `@font-face{font-family:x}`, `@keyframes k{from{}}`, `@property --x{syntax:"*";inherits:false}`, `@namespace x url(a)`, `@page{}` | **SyntaxError** | вставлено |

`<style>@layer a{#x{color:red}}</style>` → `document.styleSheets[0].cssRules.length` = 0 (то же для `@scope`, `@property`, `@container`, `@supports`, `@font-face`, `@keyframes`).

## Как найдено

WPT-RUN-14 срез 20: `css-cascade/layer-cssom-order-reverse.html`, `scope-cssom.html`, `idlharness.html` (29 из 31), `css-nesting/cssom.html` (`CSSGroupingRule is not defined`).

## Что делать

Задача `CSSOM-10` (`ROADMAP.md`): расширить реестр правил листа типами at-правил с их разбором в `insertRule` и сериализацией (`cssText`), вложенными списками правил и классами `CSSGroupingRule` и потомками.

## Как проверить

Таблица выше; `css/css-cascade/layer-cssom-order-reverse.html`, `css/css-nesting/cssom.html`.
