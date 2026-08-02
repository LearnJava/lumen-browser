# BUG-511: `link-parameters` CSS property not implemented at all

**Статус:** OPEN
**Дата:** 2026-08-03
**Компонент:** css-parser + layout (`grep -rn "link-parameters\|link_parameters"
crates/` — zero hits anywhere in the workspace)
**Найден:** WPT-RUN-3 срез 21 (`ROADMAP.md`) — массовый прогон `css/css-link-params`

## Механизм

`link-parameters` is defined by the CSS Linked Parameters Module Level 1
(`https://drafts.csswg.org/css-link-params-1/`), a very early-stage CSSWG
Editor's Draft (the `param()` value function that reads named parameters off
a `<link>` element rather than a custom property). It is not shipped in any
evergreen browser as of this engine's knowledge cutoff — filed anyway per
the same track policy as [BUG-507](BUG-507-OPEN.md) (CSS Exclusions
`wrap-flow`/`wrap-through`): a real spec gap the WPT corpus exercises, not a
regression, even though it may never ship upstream.

The property is entirely absent from `ComputedStyle` and the parser's
known-property table — `getComputedStyle(el).linkParameters` and
`.getPropertyValue('link-parameters')` both report "not supported" rather
than returning a value.

## Симптом

```
FAIL Property link-parameters has initial value none
  assert_true: link-parameters doesn't seem to be supported in the computed
  style expected true got false
FAIL Property link-parameters value 'param(--a, orange)'
  assert_true: link-parameters doesn't seem to be supported in the computed
  style expected true got false
```

## Масштаб находки

2 files / 7 subtests: `inheritance.html` (2), `link-parameters-computed.html`
(5). A third file, `link-parameters-invalid.html` (12 subtests), fails on a
different, pre-existing mechanism — the inline `style` setter accepts
malformed `param(...)` syntax instead of rejecting it — attributed to
[BUG-484](BUG-484-OPEN.md) instead, since that gap is generic to every
property, not specific to `link-parameters`. `link-parameters-valid.html`
(6/6) passes outright: setting a syntactically valid value through
`element.style` round-trips correctly even without real grammar validation,
because `_lumen_make_style` stores the input verbatim (see BUG-484).

## .ini

Committed `.ini` under `tests/wpt/metadata/css/css-link-params/` for
`inheritance.html` and `link-parameters-computed.html`, `expected: FAIL` per
subtest.
