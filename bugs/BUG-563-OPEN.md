# BUG-563: inline-style CSSOM getter/setter for `anchor()`/`anchor-size()`/`position-area` is a raw-text passthrough — no parsing, validation, or canonical serialization

**Статус:** OPEN
**Дата:** 2026-08-04
**Компонент:** js (`crates/js/src/dom.rs` — `_lumen_make_style`/`_lumen_parse_style`/`_lumen_serialize_style`, the `element.style.<prop>` getter/setter shim)
**Найден:** P2, WPT-RUN-3 срез 42 (`css/css-anchor-position`), 2026-08-04

## Симптом

Two symptoms with one root cause, both from `css/support/parsing-testcommon.js`'s `test_valid_value`/`test_invalid_value` helpers, which drive every test via `div.style[property] = value` then read back `div.style.getPropertyValue(property)`:

1. **Invalid values wrongly round-trip.** `test_invalid_value` expects `getPropertyValue` to come back `""` (property must stay unset) for a syntactically-invalid value. Lumen echoes the value back verbatim instead:
   ```
   e.style['position-area'] = "none none" should not set the property value
     assert_equals: expected "" but got "none none"
   e.style['position-area'] = "left inline-start" should not set the property value
     assert_equals: expected "" but got "left inline-start"
   ```
2. **Valid values echo in author order, not canonical order.** `test_valid_value` expects the CSSOM serialization to be canonical (spec-mandated token order) regardless of how the author wrote it:
   ```
   e.style['left'] = "anchor(inside --foo)" should set the property value
     assert_equals: serialization should be canonical expected "anchor(--foo inside)" but got "anchor(inside --foo)"
   e.style['position-area'] = "top span-self-x-start" should set the property value
     assert_equals: serialization should be canonical expected "span-self-x-start top" but got "top span-self-x-start"
   ```

Combined this explains all 4024 non-passing checks (2530 canonicalization + 1494 invalid-value-accepted) across 3 files: `anchor-parse-valid.html`, `anchor-size-parse-valid.html`, `position-area-parsing.html` — exhaustively categorized, no third message pattern left over.

## Причина

Verified by direct source read (not inferred from the WPT failures). `anchor()`/`anchor-size()`/`position-area` *are* genuinely parsed and validated — but only on the layout-resolution path, never on the CSSOM path these tests exercise:

- Real parsers exist: `crates/engine/layout/src/style.rs::parse_anchor_func` (~line 22210), `::parse_anchor_size_func` (~line 22176), `::parse_inset_area_keyword` (~line 22146), reached from `apply_declaration`'s `"top"`/`"left"`/etc. and `"position-area"`/`"inset-area"` arms (~lines 15916-15928, ~15956-15960). These correctly expect/require author order `--name side` (matching what's echoed back) and correctly reject a 3-token `position-area` value (`_ => return` guard).
- But `element.style.<prop>` never calls any of that. The getter/setter (`_lumen_make_style`, `crates/js/src/dom.rs:4260-4298`) reads/writes the raw `style` HTML attribute text via a naive `key: value` splitter (`_lumen_parse_style`/`_lumen_serialize_style`, `dom.rs:4241-4255`) — pure string passthrough, no parse step, no validation, no serialize step at all.
- No serializer exists for these types in the first place: `crates/engine/layout/src/anchor.rs`'s `AnchorFunc`/`AnchorSizeFunc`/`InsetAreaKeyword` are plain `#[derive(Debug, Clone, PartialEq)]` — no `Display`/`to_css` impl anywhere in the crate. They're consumed only by layout resolution, never turned back into a CSS string.

So both symptoms are the same gap seen from two angles: because the CSSOM path never parses at all, an invalid string is never rejected (nothing checks it), and a valid string is never re-serialized (nothing normalizes it) — it just echoes back byte-for-byte either way. A second, structurally identical raw-passthrough instance exists for `cssText` (`_serialize_style_map`/its counterpart, `dom.rs:380-401`, used around `dom.rs:3147`/`3164`) — not exercised by this bundle's failures but the same architecture, likely the same class of gap for any property whose CSSOM round-trip needs normalization, not just anchor-position's.

## Масштаб

Fix needs two parts: (1) wire `_lumen_make_style`'s setter through the existing `parse_anchor_func`/`parse_anchor_size_func`/`parse_inset_area_keyword` (or equivalent) so invalid values are rejected instead of stored, and (2) add a canonical serializer for `AnchorFunc`/`AnchorSizeFunc`/`InsetAreaKeyword` and wire the getter through it instead of raw attribute-text passthrough. Not scoped further this slice — likely P4 (parsing/serialization) + js shim wiring, not a one-line fix. The `position-area` "three keywords" invalid case (`"top left top"`) is already correctly guarded at the `apply_declaration` layer; it fails here purely because that layer is unreachable from `element.style`, not because of a second, independent hole.
