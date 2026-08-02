# BUG-517: CSS Rhythm `block-step`/`block-step-size`/`-insert`/`-align`/`-round` not implemented at all

**Статус:** OPEN
**Дата:** 2026-08-03
**Компонент:** css-parser + layout (`grep -n "\"block-step" crates/engine/css-parser/src/lib.rs
crates/engine/layout/src/style.rs` — zero hits. `line-height-step`, the
module's other property, is implemented (✅ in CSS-SPECS.md) — this is the
module's second, unrelated property family, not a partial gap in the same
one.)
**Найден:** WPT-RUN-3 срез 22 (`ROADMAP.md`) — массовый прогон `css/css-rhythm`

## Симптом

```
FAIL Property block-step-align value 'auto'
  assert_true: block-step-align doesn't seem to be supported in the
  computed style expected true got false
FAIL e.style['block-step'] = "auto" should set the property value
  assert_equals: serialization should be canonical expected "none" but
  got "auto"
```

## Механизм

`block-step`/`block-step-size`/`block-step-insert`/`block-step-align`/
`block-step-round` (CSS Rhythmic Sizing L1 §3, an Editor's Draft) are not in
the parser's known-property table at all — no parsing, no `ComputedStyle`
storage, no cascade/inheritance entry. The `e.style[...] = "..."` failures
are not a distinct gap: because the property name is unrecognized, the
generic inline-style-setter passthrough ([BUG-484](BUG-484-OPEN.md)) takes
over and stores the raw string verbatim instead of parsing+canonicalizing
it — same downstream shape as every other unimplemented-property category
in this track.

## Масштаб находки

15 files / 155 subtests, all under `css/css-rhythm/parsing/` — the entire
category (100% of its testharness ids). File per longhand:
`block-step-computed.html` (26), `block-step-valid.html` (23),
`block-step-align-invalid.html`/`-insert-invalid.html`/`-round-invalid.html`
(12/12/9 — BUG-484's setter passthrough), `block-step-size-valid.html` (1,
canonical-serialization form of the same passthrough), plus the remaining
`-align`/`-insert`/`-round`/`-size` computed/invalid files split the same
way. 0 files reach a genuinely distinct failure mode — filed as a single
bug per the module, not five, since fixing "recognize block-step* as
properties" fixes all five longhands' non-invalid subtests identically.

## Что нужно

Add `block-step`/`block-step-size`/`block-step-insert`/`block-step-align`/
`block-step-round` to the parser's known-property table (`css-parser/src/
lib.rs`), matching `ComputedStyle` fields, cascade/inheritance wiring, and
`computed_style_to_map` entries (per [BUG-472](BUG-472-OPEN.md)'s pattern).
Filed per the standing track policy of filing even very-early-draft specs
(precedent: [BUG-507](BUG-507-OPEN.md) `css-exclusions`) — Editor's Draft,
not yet shipped in any evergreen browser, low implementation priority.

## .ini

Committed `.ini` under `tests/wpt/metadata/css/css-rhythm/` for all 15
files, `expected: FAIL` per subtest.
