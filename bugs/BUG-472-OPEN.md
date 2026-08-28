# BUG-472: `getComputedStyle()` resolved-value coverage gaps

**Статус:** OPEN
**Тип:** доработка (нереализованная функциональность), не дефект — ведётся как задача [`CSSOM-3`](../ROADMAP.md) дорожки CSSOM, а не как строка очереди P3. Файл остаётся детальной записью наблюдений: «срезы» ниже — прогоны категорий WPT, упиравшиеся в эту же дыру, а не куски выполненной работы. Переклассифицировано 2026-08-28 по решению пользователя.
**Дата:** 2026-08-02
**Компонент:** layout (`crates/engine/layout/src/selector_query.rs::
computed_style_to_map`) + js (`crates/js/src/dom.rs`,
`_lumen_get_computed_style`)
**Найден:** WPT-RUN-3 срез 3 (`ROADMAP.md`) — массовый прогон `css/cssom`

## Механизм

`window.getComputedStyle(el).getPropertyValue(prop)` в шиме (`dom.rs:12772`)
делегирует нативному хуку `_lumen_get_computed_style(nid, prop)`
(`dom.rs:2525`), который — простой `HashMap` lookup без вычисления по
запросу: `computed_styles: Arc<Mutex<HashMap<u32, HashMap<String, String>>>>`,
заполняемый **один раз за проход layout** функцией
`collect_computed_styles`/`computed_style_to_map` (`selector_query.rs:590`,
ёмкость карты — жёстко заданные ~64 записи). Любое свойство/семантика вне
этого рукописного списка возвращает пустую строку — не ошибку, а тихий
`|| ''` фоллбэк (`dom.rs:12780`).

## Симптом

```
FAIL line-height: 1 - assert_equals: 1 should compute to 16px expected "16px" but got ""
FAIL Resolved value of min-width ... expected "0px" but got ""
FAIL The resolved value for 'background-color' is the used value
  - assert_regexp_match: expected object "/^rgb[a]?\(/" but got ""
FAIL Resolution of width is correct for ::before and ::after pseudo-elements
  - assert_equals: expected "50px" but got ""
```

Пустая строка вместо конкретного значения на широком спектре сценариев,
которых нет в рукописной карте `computed_style_to_map`: разрешённые
(«used») значения `top`/`right`/`bottom`/`left` для позиционированных
элементов относительно фактического layout, `min-width`/`min-height:auto` →
`0px`, computed style псевдоэлементов (`::before`/`::after`/`::picker`),
пользовательские свойства (`--x`) в computed style, `line-height` при
числовом значении, цвета, не сведённые к used-value `rgb()` (может
пересекаться с [BUG-465](BUG-465-OPEN.md), но там про сериализацию заданного
через `style.color =`, здесь — про сами эти свойства отсутствующие в карте
вовсе для отдельных сценариев типа `border-block-end-color`), computed style
у фрагментов, вставленных в IB-split.

## Масштаб находки

37 файлов массового прогона `css/cssom` (`getComputedStyle-*`/
`computed-style-*`) падают исключительно на пустых/отсутствующих значениях
из `getComputedStyle` — не на самой функции (она существует и работает для
свойств, которые в карте есть, см. существующие юнит-тесты
`get_computed_style_*` в `dom.rs`).

**WPT-RUN-3 срез 9 (`css/css-backgrounds`, 2026-08-02)** — 25 файлов / ~324
сабтеста: весь `background-attachment`/`-clip`/`-color`/`-image`/`-origin`/
`-position`/`-repeat`/`-size` computed-style кластер
(`parsing/background-*-computed.html`, `parsing/background-computed.html`,
numbered `background-33{1,2,3,5,6}.html`, `background-{size,clip,origin}-001.html`)
плюс новая грань того же гэпа: даже **физические longhand'ы**, давно
реализованные для рендеринга (`border-top-color`, `border-top-width`,
`border-top-style`, `border-top-left-radius` и парные), отсутствуют в карте
не только для составных значений border-shorthand'ов — `parsing/
border-{color,radius,style,width}-computed.html`, все 4 файла падают на
собственных физических longhand-именах, не только на shorthand'ах
(`border-color`/`border-width`/`border-style`/`border-radius`). Подтверждает
исходную гипотезу «рукописная карта ~64 записи» буквально — список
недостающих ключей шире, чем предполагалось на срезе 3.

## .ini

Committed `.ini` под `tests/wpt/metadata/css/cssom/` для всех 37
атрибутированных файлов, `expected: FAIL`/`TIMEOUT` по фактическому
результату прогона. Срез 9 добавил `.ini` под
`tests/wpt/metadata/css/css-backgrounds/` для ещё 25 файлов (некоторые
делят файл с BUG-463/BUG-495 — общий заголовок `.ini` ссылается на оба).

**WPT-RUN-3 срез 10 (`css/css-variables`, 2026-08-02)** — confirmed missing
keys (full list, checked against `computed_style_to_map` source directly,
`selector_query.rs:625-945`): `border-spacing`, `box-shadow`, `text-shadow`,
`perspective-origin`, `transition-duration`, and the entire
`background-attachment`/`-clip`/`-image`/`-origin`/`-position`/`-repeat`/
`-size` family (only `background-color` is present — `background` itself, as
well as `margin`/`padding`/`border`/`border-radius` shorthands, are likewise
absent; only their physical longhands are in the map). Most of these
co-occur with [BUG-493](BUG-493-OPEN.md) in the same files (a file testing
`border-spacing` via `var()` substitution fails at the BUG-493 layer before
this gap is even reachable — the whole per-node cache entry is empty, not
just this one key) — flagged here as the *compounding, still-present-after-
BUG-493-is-fixed* cause for: `variable-substitution-basic.html`
(`border-spacing`), `variable-substitution-background-properties.html` (7 of
its 8 properties besides `background-color`), `variable-substitution-shadow-properties.html`
(`box-shadow`/`text-shadow`), `variable-substitution-shorthands.html`
(`transition-duration`, 1 of its 51 subtests), `variable-reference-perspective-origin.html`
(`perspective-origin`), `missing-closing-nested-fallback.html` (`box-shadow`,
sole cause — this file doesn't hit BUG-493). `variable-presentation-attribute.html`
additionally exposes a **wider, SVG-specific gap**: none of ~40 SVG
presentation properties it tests (`stroke-width`, `fill`, `clip-rule`,
`dominant-baseline`, `alignment-baseline`, …) are in the map at all — same
mechanism, unmeasured beyond this one file.

## Срез 12 (`css/css-logical`, 2026-08-02) — the whole CSS Logical Properties family, confirmed absent from the map by direct source read

`grep -n "block-size\|inline-size\|inset-block\|inset-inline\|margin-block\|
margin-inline\|padding-block\|padding-inline\|border-block\|border-inline"
crates/engine/layout/src/selector_query.rs` returns **zero hits** — not a
single CSS Logical Properties longhand or shorthand name is a key in
`computed_style_to_map`, even though the physical properties they resolve
to (`width`/`height`/`top`/`right`/`bottom`/`left`/`margin-top`/…/
`border-top-color`/…) are themselves present and correctly computed
(`resolve_logical_properties`, `style.rs:8304`, converts logical → physical
on the `ComputedStyle` struct itself, upstream of the map — the map is
simply never taught the logical *names* as aliases). This is the largest
single extension of BUG-472 to date: 19 files / ~180 subtests this slice
— every `parsing/*-computed.html` file in the category
(`block-size-computed`, `inline-size-computed`, `max-/min-block-/inline-
size-computed`, `border-block-/inline-{color,style,width}-computed`,
`inset-block-inline-computed`, `inset-computed`, `margin-block-inline-
computed`, `padding-block-inline-computed`), plus `getComputedStyle-listing.html`
(all 30 subtests — the test iterates the full CSS Logical Properties list
and asserts each name is present in the resolved computed style),
`logicalprops-with-deferred-writing-mode.html` (fails on the very first
checked property, `margin-block-start`), and `logicalprops-with-variables.html`
(the `margin-inline-start`/`-end`/`margin-inline` computed-value checks —
compounded by [BUG-493](BUG-493-OPEN.md), but the map gap alone is
sufficient: even a synchronous flush wouldn't produce a value for a key
that was never inserted). `.ini` under `tests/wpt/metadata/css/css-logical/`
for all 19 files, `expected: FAIL` per subtest.

## Срез 15 (`css/css-easing`, 2026-08-02) — the whole `animation-timing-function`/`transition-timing-function` family is absent from the map

`grep -n "animation-timing-function\|animation_timing_function\|\"animation-\|\"transition-" crates/engine/layout/src/selector_query.rs` returns **zero
hits** — not one `animation-*`/`transition-*` key exists in
`computed_style_to_map`, even though the property itself parses correctly
into `ComputedStyle::animation_timing_functions`
(`crates/engine/layout/src/style.rs:3711`, `"animation-timing-function" =>`
handler at `style.rs:16562`, with passing unit tests for `linear()`/
`cubic-bezier()`/`steps()` parsing) — same "parsed correctly, never taught
to the resolved-value map" shape as `css-logical`'s срез 12 finding.
`timing-functions-syntax-computed.html` (21 subtests, 100% of the file) is a
pure, isolated repro: every one of 21 valid `animation-timing-function`
values (keywords, `cubic-bezier()`, `steps()`, `linear, ease, linear` list
form) fails identically on `assert_true: animation-timing-function doesn't
seem to be supported in the computed style expected true got false` —
`css/support/parsing-testcommon.js`'s standard feature-detect for "is this
computed-style key present at all". `linear-timing-functions-syntax.html`
contributes another 13 subtests of the same signature, restricted to
`linear(...)` values specifically. 34 subtests / 2 files this slice — the
largest single-slice addition to this bug since срез 12's CSS Logical
Properties family. `.ini` under `tests/wpt/metadata/css/css-easing/` for
both files, `expected: FAIL` per subtest.

## Срез 21 (`css/css-overscroll-behavior`, 2026-08-03) — physical `overscroll-behavior-x`/`-y` parse and store correctly but never reach the map

`grep -n "overscroll" crates/engine/layout/src/selector_query.rs` returns
zero hits, while `style.rs:3644-3645`/`16347-16361` confirm
`overscroll_behavior_x`/`_y` are real `ComputedStyle` fields, parsed and
stored by `overscroll-behavior`/`-x`/`-y` declarations — same
"parsed-but-never-taught-to-the-map" shape as срезы 12/15. 12 subtests / 2
files: `inheritance.html` (4 — initial-value/does-not-inherit checks for
`-x`/`-y`), `parsing/overscroll-behavior-computed.html` (8 — 4 values ×
`-x`/`-y`). The logical `overscroll-behavior-block`/`-inline` half of these
same two files is a *different*, deeper gap — the properties aren't
recognized by the parser at all, not merely missing from this map — filed
separately as [BUG-516](BUG-516-OPEN.md). `.ini` under
`tests/wpt/metadata/css/css-overscroll-behavior/` for both files.

## Срез 22 (`css/css-color-adjust`, 2026-08-03) — `color-scheme`/`color-adjust`/`forced-color-adjust`/`print-color-adjust` all parse and store but never reach the map

`grep -n "color-scheme\|color-adjust" crates/driver/src/*.rs crates/js/
src/*.rs` for `computed_style_to_map`-style insertions returns zero hits,
while `layout/src/style.rs:14538/14561/15313/18420/18491` confirm all four
properties are parsed and applied to `ComputedStyle` — same
"parsed-but-never-taught-to-the-map" shape as срезы 12/15/21. 21 subtests /
2 files: `inheritance.html` (8 — initial-value/inherits checks for all four
properties), `parsing/color-scheme-computed.html` (13 — every valid
`color-scheme` value). `color-scheme-root-background.html`'s single fail
(`expected "rgba(0, 0, 0, 0)" but got ""` for the root element's UA
background under a dark scheme) is left unattributed this slice — plausibly
the same map gap surfacing through `background-color`, plausibly a real
rendering gap (dark-scheme UA stylesheet override not applied at all); not
isolated further. `.ini` under `tests/wpt/metadata/css/css-color-adjust/`
for both files.

## Срез 23 (`css/fill-stroke`, 2026-08-03) — `fill`/`stroke`-and-friends SVG paint properties parse and store but never reach the map

`grep -n '"fill"\|"stroke"\|"stroke-color"\|"fill-opacity"\|"stroke-width"'
crates/engine/layout/src/selector_query.rs` returns zero hits, while
`layout/src/style.rs:3926-3952` confirm `svg_fill`/`svg_fill_opacity`/
`svg_stroke`/`svg_stroke_opacity`/`svg_stroke_width`/`svg_fill_rule`/
`svg_stroke_linecap`/`svg_stroke_linejoin`/`svg_stroke_miterlimit`/
`svg_stroke_dasharray`/`svg_stroke_dashoffset` are all real `ComputedStyle`
fields — same "parsed-but-never-taught-to-the-map" shape as срезы 12/15/
21/22. 220 subtests / 2 files, entirely `css/support/interpolation-
testcommon.js`'s standard "is this computed-style key present at all"
feature-detect, all failing `assert_true: Web Animations should be
supported`/`'to'/'from' value should be supported expected true got
false`: `animation/fill-interpolation.html` (48) and `animation/
stroke-color-interpolation.html` (172, the largest single-file
contribution to this bug to date). The category's other two properties,
`text-decoration-fill`/`text-decoration-stroke`/`-webkit-text-stroke`, are
a different and deeper gap — not parsed or stored at all, filed separately
as [BUG-521](BUG-521-OPEN.md). `.ini` under
`tests/wpt/metadata/css/fill-stroke/animation/` for both files.

## Срез 24 (`css/css-scroll-anchoring` + `css/css-content`, 2026-08-03) — `overflow-anchor`, `quotes`, `bookmark-level`/`-state`, `content` never reach the map

Same "parsed-but-never-taught-to-the-map" shape (`computed_style_to_map`
doesn't cover these keys), though `overflow-anchor` is a special case — it
isn't parsed at all (filed separately as
[BUG-524](BUG-524-OPEN.md)), so its "doesn't seem to be supported" failure
is really BUG-524, not this bug (kept off this bug's file count). Real
BUG-472 extensions this slice: `quotes`/`bookmark-level`/`bookmark-state`
(`css-content/inheritance.html`, 6 subtests) and `content` itself
(`css-content/computed-value.html` + `css-content/parsing/content-computed.html`,
42 subtests) — `content` is a real, working property (generates pseudo-
element content), just never surfaced through `getComputedStyle()`. `.ini`
under `tests/wpt/metadata/css/css-content/` for these files.

## Срез 24 (`css/compositing`, 2026-08-03) — `background-blend-mode`, `isolation`, `mix-blend-mode`

Same "parsed-but-never-taught-to-the-map" shape. 45 subtests/5 files:
`inheritance.html` (6), `parsing/background-blend-mode-computed{,-multiple}.html`
(19+7), `parsing/isolation-computed.html` (2),
`parsing/mix-blend-mode-computed.html` (16). `.ini` under
`tests/wpt/metadata/css/compositing/`.

## Срез 24 (`css/css-scrollbars`, 2026-08-03) — `scrollbar-color`, `scrollbar-width`

Same "parsed-but-never-taught-to-the-map" shape, 4 subtests
(`inheritance.html`). `.ini` under `tests/wpt/metadata/css/css-scrollbars/`.

## Срез 26 (`css/css-ruby` + `css/css-page`, 2026-08-03) — `ruby-*`, `page`

Same shape, two categories: `css-ruby/inheritance.html` (8 subtests —
`ruby-align`/`ruby-position`/`ruby-merge`/`ruby-overhang`, all four "has
initial value"/"inherits" pairs); `css-page/inheritance.html` (2) +
`css-page/parsing/page-computed.html` (6) — the `page` property. `.ini`
under `tests/wpt/metadata/css/css-ruby/` and
`tests/wpt/metadata/css/css-page/`.
