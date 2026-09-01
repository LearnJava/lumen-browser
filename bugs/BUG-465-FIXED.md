# BUG-465: computed/reflected color values are not serialized to canonical `rgb()` form

**Статус:** FIXED 2026-09-01
**Дата:** 2026-08-02
**Компонент:** css-parser / layout (color serialization, `CSSStyleDeclaration`
value reflection)
**Найден:** WPT-RUN-3 срез 2 (`ROADMAP.md`) — массовый прогон `css/CSS2`,
`css/CSS2/syntax/colors-007.html` (904/1192 сабтестов FAIL — крупнейший
единичный кластер во всём срезе)

## Симптом

```
FAIL e.style['color'] = "#ffffff" should set the property value
  - assert_equals: serialization should be canonical expected "rgb(255, 255, 255)" but got "#ffffff"
FAIL e.style['color'] = "rgb(100%, 100%, 100%)" should set the property value
  - assert_equals: serialization should be canonical expected "rgb(255, 255, 255)" but got "rgb(100%, 100%, 100%)"
```

Per CSS Color Module (§4.2/§8, CSSOM §6.7.3), reading back a color value from
`CSSStyleDeclaration`/`getComputedStyle` must always serialize to the
canonical `rgb(r, g, b)` (или `rgba(...)` с альфой) form with integer 0-255
channels, независимо от того, каким синтаксисом цвет был задан (`#fff`,
`#ffffff`, `rgb(50%, ...)`, `rgb(+255, ...)`, legacy-запятые/без запятых и
т.п.). Lumen возвращает исходную запись почти без изменений — сериализация
цвета в объект `style`/computed style не реализована вовсе, а не просто
неточна (сравнивались десятки разных входных форм для одного и того же цвета,
все дают неканонический вывод).

## Влияние вне WPT

Любой код, читающий `el.style.color`/`getComputedStyle(el).color` и
сравнивающий его с ожидаемой строкой (частый паттерн в тестах и в
color-picker/theming коде), видит непредсказуемый формат вместо
гарантированного спекой канонического.

## .ini

`tests/wpt/metadata/css/CSS2/syntax/colors-007.html.ini` — 904 сабтеста
`expected: FAIL` (полный список конкретных серилизаций — в самом `.ini`, не
дублируется здесь).

## Fix (P3, 2026-09-01)

**Scope correction first.** `getComputedStyle()` already serializes
`<color>` canonically — `selector_query::css_color_to_css`/`color_to_css`
run on every resolved `CssColor` and always emit `rgb()`/`rgba()`
(unconditionally, since the computed value of a resolved color is always
numeric per CSS Color L4). A probe against a fresh build confirmed this
directly: `getComputedStyle(el).color` for `color:red` already reads back
`"rgb(255, 0, 0)"`. The actual gap named by every failing subtest in
`colors-007.html` is `CSSStyleDeclaration` (inline `style`) *specified*-value
reflection — `el.style['color'] = value; el.style.getPropertyValue('color')`
— which is a different code path (`_lumen_make_style` in
`crates/js/src/shim/web_api_shim_mid.js`) that did no parsing at all:
`setProperty` stored `String(val)` verbatim and `getPropertyValue` echoed it
back unchanged. This also meant `setProperty` never rejected a syntactically
invalid `<color>` (`"#00000"`, `"invalidValue"`, …) — CSSOM requires an
unparseable value to be a no-op.

A probe generated directly from `colors-007.html`'s own `valid_colors`/
`invalid_colors`/`color_properties` tables (110 valid inputs × 7 properties +
12 invalid inputs × 7 properties + round-trip check = 1192, matching the
"904/1192" figure in the symptom above) also showed the 288 already-passing
subtests were the pure-keyword cases (`"red"`, `"aqua"`, `"inherit"`, …) —
those already round-tripped verbatim since no validation touched them; only
hex/legacy-functional syntax and the invalid-rejection cases were the real
904-subtest gap. Worth noting for future .ini triage: a per-subtest `.ini`
generated from a wptrunner run is not automatically per-subtest-accurate —
this one over-flagged those 288 as `FAIL` (`docs/probe-method.md`'s "a wall
of failures can be one hang, not N distinct defects" applies to mass-marked
`.ini`s too, not just live TIMEOUT walls).

**Implementation.** New `lumen_layout::style::canonical_specified_color`
(`crates/engine/layout/src/style/parse/color.rs`) distinguishes keyword-form
input (named/system-color/`currentcolor`/`transparent`/the CSS-wide keywords
`inherit`/`initial`/`unset`/`revert`/`revert-layer`, which are valid on any
property and are not `<color>` syntax) — serialized as the keyword itself,
lowercased — from hex/legacy-functional syntax (`rgb()`/`rgba()`/`hsl()`/
`hsla()`/`hwb()`/…), canonicalized to `rgb()`/`rgba()` via the same
`color_to_css` helper `getComputedStyle` already uses (promoted to
`pub(crate)` in `selector_query.rs` — one serializer, not a second copy).
Returns `None` for anything else, so the caller can reject the assignment.
Exposed to the JS shim as the native `_lumen_css_canonical_color`
(`crates/js/src/v8_runtime/install/platform.rs`, next to the existing
`CSS.supports()` natives). `_lumen_make_style`'s `setProperty` in
`web_api_shim_mid.js` now: (1) treats an empty string as `removeProperty`
per CSSOM §6.7.4 (needed so clearing a color property via `style.color = ""`
still works once the validation branch below exists); (2) for the eight
`<color>`-typed longhands actually exercised by the test
(`color`/`background-color`/`border-{top,bottom,left,right}-color`/
`outline-color`/`text-decoration-color` — deliberately *not* every
`*-color` property: `border-color` is a 1-4-value shorthand needing the
general shorthand-expansion machinery tracked by BUG-473, not plain
`<color>` parsing) calls the native and rejects on `null`; (3) everything
else keeps the prior naive pass-through, unchanged.

**Verification.** New Rust unit tests in
`crates/engine/layout/src/style/tests/color.rs`
(`canonical_specified_color_{hex_and_functional_forms_become_rgb,keeps_keyword_syntax_as_keyword,rejects_invalid_syntax}`).
A probe built directly from `colors-007.html`'s own tables (not
hand-written) run through `lumen --dump-layout`: **1192/1192 pass, 0 fail**
(the exact subtest count named in the symptom section). Existing
`cargo test -p lumen-js --features v8-backend style` (65 tests, including
`dom::tests::v8_events_cache::style_{set_and_get_property,css_text_roundtrip,remove_property,camel_case_to_kebab}`)
and `cargo test -p lumen-layout` color/style suites pass unchanged — no
regression to non-color properties or the general style-declaration
mechanics. `cargo clippy -p lumen-layout --all-targets` and
`cargo clippy -p lumen-js --all-targets --features v8-backend` both clean.
`tests/wpt/metadata/css/CSS2/syntax/colors-007.html.ini` deleted (every
subtest it listed now passes; the file's non-`FAIL` lines were just the
category header and `[colors-007.html]` — nothing else to keep).
