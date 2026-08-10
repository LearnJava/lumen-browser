# BUG-594: `hidden` attribute reflects as plain boolean -- `hidden="until-found"` mode entirely unimplemented (no reveal algorithm, no `beforematch` event)

**Статус:** OPEN
**Компонент:** js (`crates/js/src/dom.rs:10724-10735`, global-attributes reflection table -- `['hidden', 'hidden', 'bool']`)
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
(`dom.rs:10728`) plus the fully self-contained `hidden-until-found-idl.html`
(no `testdriver`, no cross-file dependency) reproducing the reflection defect
in isolation.
