# BUG-521: `text-decoration-fill`/`text-decoration-stroke`/`-webkit-text-stroke` not implemented at all

**Статус:** OPEN (ДОРАБОТКА → CSS-SPECS.md)
**Тип:** never-implemented feature family, not a defect in existing code — tracked in `CSS-SPECS.md` going forward, not taken by P3's bug queue
**Дата:** 2026-08-03
**Компонент:** css-parser/layout (no trace anywhere:
`grep -rn "text-decoration-fill\|text-decoration-stroke\|webkit-text-stroke"
crates/engine/css-parser/src/*.rs crates/engine/layout/src/style.rs` — zero
hits)
**Найден:** WPT-RUN-3 срез 23 (`ROADMAP.md`) — массовый прогон `css/fill-stroke`

## Механизм

CSS Fill and Stroke Module L4 defines two families: the core `fill`/
`stroke`-and-friends properties (already parsed and stored on
`ComputedStyle` as `svg_fill`/`svg_stroke`/etc. — see
[BUG-472](BUG-472-OPEN.md)'s срез 23 extension for that half), and a
separate, unrelated family that lets *text* itself be filled/stroked:
`text-decoration-fill`, `text-decoration-stroke` (the spec-current pair)
and the legacy `-webkit-text-stroke`/`-webkit-text-stroke-color`/
`-webkit-text-stroke-width` alias. Neither the parser's known-property
table nor `ComputedStyle` has any field for any of these — they are not a
"parsed but not exposed" gap like BUG-472, they are entirely unrecognized
declarations (silently dropped by the cascade like any unknown property).

## Симптом

`css/fill-stroke/inheritance.html`:
```
FAIL Property text-decoration-fill has initial value match-text
  - assert_true: text-decoration-fill doesn't seem to be supported
    in the computed style expected true got false
FAIL Property text-decoration-fill does not inherit - assert_true: expected true got false
FAIL Property text-decoration-stroke has initial value match-text - assert_true: ...
FAIL Property text-decoration-stroke does not inherit - assert_true: expected true got false
```
`css/fill-stroke/webkit-text-stroke-computed.html`:
```
FAIL Property -webkit-text-stroke value 'green' - assert_true: -webkit-text-stroke
  doesn't seem to be supported in the computed style expected true got false
FAIL Property -webkit-text-stroke value '3px' - assert_true: ... (same)
FAIL Property -webkit-text-stroke value '1px red' - assert_true: ... (same)
```

## Масштаб находки

2 files / 7 subtests, both testharness ids in `css/fill-stroke` that touch
this property family (the interpolation tests in the same category exercise
the *other* half, `fill`/`stroke-color`, tracked under BUG-472 instead).

## .ini

Committed `.ini` under `tests/wpt/metadata/css/fill-stroke/` for
`inheritance.html` and `webkit-text-stroke-computed.html` (both fully
attributed to this bug, 4/4 and 3/3 subtests respectively),
`expected: FAIL`.

## Ревизия P3 2026-09-06

Reclassified from bug to ДОРАБОТКА per `docs/probe-method.md` §8: the
functionality is absent outright (confirmed above) *and* the missing half is
a rendering algorithm, not a point patch. Lumen has two independent text
render paths (`crates/engine/paint/src/cpu_raster.rs` and
`crates/engine/paint/src/backends/femtovg_backend.rs`); neither has any glyph
outline/stroke capability today — both fill the glyph shape only (`grep -rn
"stroke" crates/engine/paint/src/cpu_raster.rs
crates/engine/paint/src/backends/femtovg_backend.rs` matches nothing about
text). Implementing `text-decoration-fill`/`text-decoration-stroke`/
`-webkit-text-stroke` for real (not just parsed-and-ignored) means designing
glyph-outline stroking in both backends independently — the same shape as
`border-image`'s missing 9-slice paint algorithm (BUG-492). Moved to
`CSS-SPECS.md`'s CSS Fill & Stroke row; removed from `STATUS-P3.md`.
