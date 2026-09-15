# BUG-594: `hidden` attribute reflects as plain boolean -- `hidden="until-found"` mode entirely unimplemented (no reveal algorithm, no `beforematch` event)

**Статус:** FIXED 2026-09-16 (P3)
**Компонент:** js (`crates/js/src/shim/web_api_shim_tail_b.js`, global-attributes reflection table -- было `['hidden', 'hidden', 'bool']`)
**Найден:** P2, WPT-VENDOR-html-editing, 2026-08-04

## Симптом

```
FAIL div.hidden = "until-found" - assert_equals: div.hidden = "until-found" should return "until-found" expected (string) "until-found" but got (boolean) true
FAIL div.hidden = "UnTiL-FoUnD" - assert_equals: ... expected (string) "until-found" but got (boolean) true
FAIL element.hidden should return "until-found" regardless of uppercase letters. - assert_equals: expected (string) "until-found" but got (boolean) true
```
(`the-hidden-attribute/hidden-until-found-idl.html`, 4/4 subtests fail)

Plus a cluster of TIMEOUT/NOTRUN across `the-hidden-attribute/beforematch-*.html`,
`hidden-until-found-002.html`, `hidden-until-found-and-details.html`,
`hidden-until-found-text-fragment.html` (the `beforematch` event never fires,
so nothing wakes the test up) -- some of these overlap with the unrelated
named-access gap [BUG-384](BUG-384-FIXED.md) (`a1 is not defined` etc.), which
masks part of the signal in this cluster the same way it did in `focus`.

## Причина

HTML LS §3.2.6.2 makes `hidden` a **tristate enumerated attribute**
(`""`/`"hidden"` → `hidden`, `"until-found"` → `until-found`, missing →
`visible`) with an IDL getter that returns one of the strings `""`/`"hidden"`/
`"until-found"` (not a plain boolean) and setter accepting either a boolean or
those strings. `_lumen_install_reflection`'s `'bool'` mode coerces the
attribute presence to a JS `true`/`false`, which is correct for `inert` and
most global booleans but wrong for `hidden` since the 2023 `until-found`
addition. There is also no reveal algorithm at all: the CSS UA stylesheet
rule `[hidden="until-found"] { content-visibility: hidden }`-equivalent
behavior, the `beforematch` event, and the "reveal ancestors on
fragment-navigation / find-in-page / `Element.focus()`" steps that HTML LS
§3.2.6.2 mandates for `until-found` are absent from the codebase (`grep -rn
beforematch crates/` and `grep -rn until-found crates/` both zero-hit outside
this table entry).

## Масштаб

13 files in `the-hidden-attribute/`, all touching this gap in one form or
another (attribute reflection, `beforematch` dispatch, or the reveal-on-
navigation algorithm). Confirmed root cause via direct code read
(`dom.rs:10728`, since split into `web_api_shim_tail_b.js`) plus the fully
self-contained `hidden-until-found-idl.html` (no `testdriver`, no cross-file
dependency) reproducing the reflection defect in isolation.

## Исправлено 2026-09-16 (P3)

Added a fourth reflection kind, `'tristate-bool'`, next to `bool`/`enum`/
`long`/`url` in `_lumen_define_reflection`
(`crates/js/src/shim/web_api_shim_tail_b.js`), and switched the `hidden`
table entry to it:

- **Getter** -- `false` if the attribute is absent; `"until-found"` if the
  attribute value is an ASCII case-insensitive match for `until-found`;
  `true` otherwise (any other present value, including `""`/`"hidden"`/
  `"false"`/`"foo"`).
- **Setter** -- if the given value is a JS string that lowercases to exactly
  `until-found`, set the content attribute to `"until-found"` verbatim
  (rejects Turkish-dotless-ı lookalikes, matching `hidden-idl.html`'s
  `untıl-found` case); otherwise fall back to `ToBoolean(v)` (numbers,
  `null`/`undefined`, empty/non-empty strings) -- `true` sets the attribute
  to `""`, `false` removes it. This is the same coercion the old `'bool'`
  mode already had for real booleans, just reordered so the `until-found`
  string check runs first.

This closes all 4 `hidden-until-found-idl.html` subtests and the tristate
half of `hidden-idl.html` (`runPropertyTest`/`runAttributeTest` cases through
`until-found`, plus the pre-existing boolean/number/null cases, which keep
passing since `ToBoolean` reproduces the old `'bool'` mode's presence
semantics exactly). 7 new Rust-side tests in
`crates/js/src/dom/tests/v8_details_dialog_popover.rs` (`hidden_getter_*`/
`hidden_setter_*`) exercise the getter/setter pairs directly through V8.
`cargo test -p lumen-js --profile dev-release --features v8-backend hidden_`
green (7/7); `cargo clippy -p lumen-js --all-targets --features v8-backend --
-D warnings` clean.

**Остаток -- не тронуто этим срезом:** the reveal algorithm (HTML LS
§3.2.6.2: reveal ancestors on fragment-navigation / find-in-page /
`Element.focus()`), the CSS UA-stylesheet `[hidden="until-found"] {
content-visibility: hidden }`-equivalent rule, and the `beforematch` event
are still entirely absent (`grep -rn beforematch crates/` and `grep -rn
until-found crates/` outside this one table entry are still zero-hit). That
is a real architectural gap (needs `content-visibility` support, a new event,
and hooking into fragment navigation / find-in-page / `.focus()`), not a
one-line reflection bug like this one was -- filed separately as
[GAP-BEFOREMATCH](../ROADMAP.md), which covers the remaining ~9 files in
`the-hidden-attribute/` (`beforematch-*.html`, `hidden-until-found-00{1,2,4,5,7}.html`,
`hidden-until-found-and-details.html`, `hidden-until-found-text-fragment.html`).
