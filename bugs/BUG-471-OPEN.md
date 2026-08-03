# BUG-471: CSSOM stylesheet/rule object model not implemented

**Статус:** OPEN
**Дата:** 2026-08-02
**Компонент:** js (`crates/js/src/dom.rs`)
**Найден:** WPT-RUN-3 срез 3 (`ROADMAP.md`) — массовый прогон `css/cssom`

## Симптом

```
TypeError: Cannot read properties of undefined (reading 'cssRules')
    at ... document.styleSheets[0].cssRules
FAIL CSSStyleSheet is not defined
TypeError: Cannot read properties of undefined (reading 'appendChild')
    at ... document.getElementById("styleElement").sheet.insertRule(...)
```

`document.styleSheets` — не пустой массив, а `undefined`; `<style>`/`<link
rel=stylesheet>` не имеют свойства `.sheet`; глобальные конструкторы
`CSSStyleSheet`, `CSSRule`, `CSSStyleRule`, `CSSGroupingRule`,
`CSSMediaRule`, `CSSNamespaceRule`, `CSSKeyframesRule`, `CSSKeyframeRule`,
`CSSPageRule`, `CSSFontFaceRule`, `CSSFontFeatureValuesRule`, `MediaList`
отсутствуют вовсе. `grep -n "styleSheets\|CSSStyleSheet\|cssRules" crates/js/
src/dom.rs` даёт ноль совпадений — не сломанный геттер, вся ветка CSSOM
(`https://drafts.csswg.org/cssom/`) не подключена к шиму. `insertRule`/
`deleteRule`, конструируемые таблицы стилей (`new CSSStyleSheet()`),
`HTMLLinkElement.disabled`/alternate-переключение и порядок
`document.styleSheets` от того же корня зависят и падают тем же образом.

## Масштаб находки

Крупнейший единичный кластер массового прогона `css/cssom`: из 178 файлов с
непройденными сабтестами **97** используют `document.styleSheets`/`.sheet`/
`CSSStyleSheet`/`CSSRule`-иерархию/`MediaList`/`insertRule`/`deleteRule`
где-либо в теле теста (подтверждено статическим сканом исходников тестов, не
оценкой по частоте строк в логе) — полный список файлов см. в шапках
committed `.ini` под `tests/wpt/metadata/css/cssom/`.

**WPT-RUN-3 срез 6 (`css/css-cascade`, 2026-08-02)**: тот же root cause
всплыл в 4 дополнительных файлах — `layer-stylesheet-multi-adoption.html`
(`new CSSStyleSheet()` конструктор отсутствует), `layer-replaceSync-clears-
stale.html` (то же — `CSSStyleSheet is not defined`), `layer-rules-
cssom.html` (`CSSLayerBlockRule`/`CSSLayerStatementRule` — те же недостающие
CSS Cascade Layers-специфичные подклассы `CSSRule`, `assert_implements:
undefined` на каждом из 9 сабтестов), `unset-value-storage.html`
(`document.styleSheets[0]` — `Cannot read properties of undefined (reading
'0')`, тот же корень: `styleSheets` не массив, а `undefined`). Committed
`.ini` под `tests/wpt/metadata/css/css-cascade/` для этих 4 файлов.

## Влияние вне WPT

`document.styleSheets`/`CSSStyleSheet`/`CSSRule` — стандартный способ
программного чтения и модификации CSS со страницы (theming-движки,
CSS-in-JS библиотеки с constructable stylesheets, dev-инструменты). Полное
отсутствие этой ветки API — не единичный дефект, а целый неподключённый
раздел спеки.

## .ini

Committed `.ini` под `tests/wpt/metadata/css/cssom/` для всех 97
атрибутированных файлов, `expected: FAIL`/`TIMEOUT` по фактическому
результату прогона.

**WPT-RUN-3 срез 10 (`css/css-variables`, 2026-08-02)** — 2 more subtests,
same root cause: `variable-invalidation.html`'s "css rule test"/"css rule
test important" both do `document.styleSheets[0].cssRules[0].style` —
`Cannot read properties of undefined (reading '0')` (`styleSheets[0]` is
`undefined`); `variable-reference.html`'s "Variable reference left open at
end of stylesheet" does `document.getElementById(id).sheet.cssRules[0]` —
same failure, `<style>`'s `.sheet` property. `.ini` for both files cites
BUG-471 for these specific subtests (each file's remaining subtests are
[BUG-484](BUG-484-OPEN.md) instead — see that bug's own extension).

**WPT-RUN-3 срез 12 (`css/css-logical`, 2026-08-02):** `logicalprops-quirklength.html`
does `document.styleSheets[0].cssRules[0]` at the top of its
`isValidDeclaration` helper, called from every one of its 10 subtests —
`document.styleSheets` is `undefined`, so `[0]` throws `Cannot read
properties of undefined (reading '0')` on the first access, before the
helper's own logic (mutate `cssRule.style`, check `.length`) ever runs.
`.ini`: `tests/wpt/metadata/css/css-logical/logicalprops-quirklength.html.ini`
(`expected: FAIL` per subtest).

## Срез 22 (`css/css-mixins`, 2026-08-03)

Largest single-category extension of this bug to date: 135 subtests across
6 files — `functions/at-function-cssom.html` (33, the dedicated
`CSSFunctionRule`/`CSSFunctionDescriptors` CSSOM surface test),
`functions/at-function-parsing.html` (~39, every `@function` grammar
variant checked via `document.styleSheets[0].cssRules[0]`),
`mixins/mixin-cssom.tentative.html` (6), `mixins/mixin-invalidation.
tentative.html` (3, `ss.cssRules[0].cssRules[0]` — confirms the gap is not
just top-level `cssRules` but the whole nested-rule chain), `mixins/
mixin-parsing.html` (14, `@layer`/style-rule/`@media`/`@supports`/
`@container`/`@starting-style`/`@scope` validity checks *inside* a
`@mixin` body, all routed through the same missing `CSSStyleSheet`
global), `mixins/mixin-shadow-dom.html` (1 of 3 — "access to mixins from
adopted stylesheets" needs `new CSSStyleSheet()`; the other 2 subtests in
that file are [BUG-518](BUG-518-OPEN.md), mixins not applying at all).
`.ini` under `tests/wpt/metadata/css/css-mixins/` for these 6 files.

## Срез 19 (`css/css-nesting`, 2026-08-03)

6 files, 45 subtests + 1 file-level `TIMEOUT` — CSS Nesting's own CSSOM
surface (`CSSNestedDeclarations`, sub-`cssRules` on a `CSSStyleRule` for its
nested rules, `insertRule`/`deleteRule`/`selectorText` on nested rules) is
entirely unreachable through this gap, same root cause as every prior slice
(`document.styleSheets` is `undefined`, `CSSStyleRule`/`CSSStyleSheet`
globals don't exist). `parsing.html` is the category's most informative
instance of the failure mode already on file for BUG-471 (top-level,
non-`test()`-wrapped access throws before the harness ever registers a
subtest, so wptrunner reports `TIMEOUT` with zero subtests rather than a
FAIL list) — its very first line is `let [ss] = document.styleSheets`,
destructuring `undefined`. `.ini` under `tests/wpt/metadata/css/css-nesting/`
for all 6 files.

## Срез 25 (`css/css-properties-values-api`, 2026-08-03)

Confirmed live with a minimal `--mcp-live-port` probe (not just static grep):
`document.styleSheets` — `typeof` `"undefined"`; a freshly created and
`document.body.append()`-ed `<style>` node's `.sheet` — also `typeof`
`"undefined"`. 6 files/168 subtests, all via the category's shared
`resources/utils.js::with_style_node`/`with_at_property` helper
(`node.sheet.rules[0]` inside a `try`-free callback, so the throw surfaces
as the assertion failure, not a crash): `at-property.html` (106, the
category's largest single-file finding), `at-property-cssom.html` (39),
`at-property-stylesheets.html` (5), `at-property-typedom.html` (2),
`determine-registration.html` (15), `registered-property-cssom.html`
(file-level `TIMEOUT` — `document.styleSheets[0].cssRules[0].style` at
top-level script scope, before any `test()` registers). `.ini` under
`tests/wpt/metadata/css/css-properties-values-api/` for all 6 files.

## Срез 26 (`css/css-page`, 2026-08-03)

Dominant finding of the category: `document.getElementById("sheet").sheet`/
`document.styleSheets` both `undefined`, hit at **top level** in most files
(`var sheet = document.getElementById("sheet").sheet; ... sheet.rules[0]`)
→ TIMEOUT before the first `test()` registers. 10 files this slice:
`cssom/margin-001.html` (4 subtests, harness OK — throw happens inside
`test()` here), `cssom/margin-002.html`/`margin-003.html` (top-level,
TIMEOUT), `cssom/page-001.html` (4, harness OK), `cssom/page-002.html`
(top-level, TIMEOUT), `page-rule-declarations-000/001/003/004.html`
(`document.styleSheets.length` at top level, TIMEOUT ×4),
`parsing/nested-rules-001.html`, `parsing/page-rules-001.html` (partial —
also hits [BUG-485](BUG-485-OPEN.md)'s `document.head.append` on other
subtests), `parsing/size-valid.html`. `.ini` under
`tests/wpt/metadata/css/css-page/`.
