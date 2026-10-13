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

## Повторное измерение: WPT-RUN-14 срез 22 (2026-10-08)

Не только `@layer`/`@scope`: `CSSStyleSheet.replaceSync('@container STYLE(--foo: bar){}')` и `insertRule('@container …')`/`@supports`/`@property`/`@function`/`@mixin`/`@view-transition` дают пустой `cssRules` или `SyntaxError`; глобалов `CSSConditionRule`, `CSSSupportsRule`, `CSSContainerRule`, `CSSPropertyRule`, `CSSFunctionRule`, `CSSViewTransitionRule`, `CSSGroupingRule` нет. Здесь же: `[...sheet.cssRules]` не итерируется (BUG-1506). Затронуто в срезе 22: `container-queries/at-container-{style-,scroll-state/}*` (22 ERROR на `assert_implements_style_container_queries`), `css-mixins/*/…cssom|parsing`, `css-properties-values-api/at-property-cssom`, `css-conditional/js/*`, `css-view-transitions/navigation/at-rule-cssom` — не меньше 40 id.

## Срез 25 (2026-10-08, P2, WPT-RUN-14 `css/css-typed-om` + `cssom-view` + `cssom` + `css-lists` + `css-counter-styles`)

`css/cssom`: 26 id по at-правилам (`CSSKeyframesRule`, `CSSFontFaceRule`, `CSSImportRule`, `CSSNamespaceRule`, `CSSPageRule`, `CSSConditionRule`, `CSSContainerRule`, `CSSGroupingRule`, `insertRule` для `@namespace`/`@import`; 166 из 179 сабтестов) + `css-counter-styles`: `typeof CSSCounterStyleRule` — `undefined`, `cssRules[0]` для `@counter-style foo{…}` — `undefined` (`counter-style-at-rule/*-syntax.html`, `cssom/*` — 29 id, `idlharness.html` 33 из 37). Проба: `<style>@font-face{…} @keyframes k{…} @import url(x.css); @page{margin:1px}</style>` — `sheet.cssRules` пуст. Разбор — `docs/wpt-vendor-notes/css.md` §css-typed-om + cssom-view + cssom + css-lists + css-counter-styles.

## Повторное измерение: WPT-RUN-14 срез 26 (2026-10-09)

`<style>@page{size:A4;margin:1in} @page foo{margin:2in} @page :first{margin:3in}</style>` — `sheet.cssRules.length` 0; `@keyframes k{…} @starting-style{div{opacity:0}} @counter-style c{…}` — `cssRules.length` 0; `typeof CSSPageRule`, `CSSMarginRule`, `CSSKeyframesRule`, `CSSStartingStyleRule`, `CSSCounterStyleRule` — `undefined`. Новые id: `css-page` 13 (`cssom/*`, `page-rule-declarations-*`, `parsing/margin-rules-*`: `rules is not iterable`, `Cannot read properties of undefined (reading 'cssRules')`), `css-transitions` 3 (`starting-style-rule-none`, `starting-style-cascade`: `Prerequisite: @starting-style parses`; `parsing/starting-style-parsing`), `css-animations` 2 (`keyframes-rule-caching`, `KeyframeEffect-setKeyframes`: `insertRule` бросает `SyntaxError`), `css-shadow` 3 (`host-parsing`, `slotted-parsing`, `host-context-parsing`: `insertRule … the index provided is larger than the maximum index`, 36 + 7 сабтестов).
