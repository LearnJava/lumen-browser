# BUG-537: `getComputedStyle()` never exposes `background-image`/`object-fit`/`object-position`/`image-rendering`/`vertical-align` (implemented in `ComputedStyle`, just missing from the serialization whitelist)

**Статус:** OPEN
**Дата:** 2026-08-03
**Компонент:** layout (`crates/engine/layout/src/selector_query.rs::computed_style_to_map`)
**Найден:** WPT-RUN-3 срез 28 (`ROADMAP.md`) — массовый прогон `css/css-images`

## Механизм

`computed_style_to_map` (`selector_query.rs:625`) builds the fixed
property→string whitelist that `window.getComputedStyle()` reads from
(`_lumen_get_computed_style`, `crates/js/src/dom.rs:2525`, looks up
`cs.lock().get(&nid).and_then(|m| m.get(&prop))`, defaulting to `""` on a
miss). `background-color` is inserted (`selector_query.rs:718`), but no other
`background-*` longhand is — confirmed by `grep -n '"background' selector_query.rs`
returning exactly one line. `ComputedStyle.background_image` is fully parsed
and paint-correct (`crates/engine/layout/src/style.rs::parse_background_gradient`
et al., consumed by `paint`/`display_list.rs`) — this is a getComputedStyle
serialization gap, not a parsing or rendering defect.

Live probe (`--mcp-live-port`, `getComputedStyle(el).getPropertyValue('background-image')`
after `el.style.backgroundImage = 'linear-gradient(black, white)'` +
`document.body.appendChild(el)`) returns `""` unconditionally — reproduced for
both spec-valid and spec-invalid gradient functions alike, confirming the gap
is unconditional (not gradient-grammar-specific).

## Симптом

`css/css-images/gradient/color-stops-parsing.html` (color-stops-parsing test,
216 subtests, one per `{gradient-function, stop-list, expected parse}` triple)
runs `check_gradient()`, which asserts twice per case: first that
`div.style.getPropertyValue('background-image')` round-trips (inline echo —
always passes independently, [BUG-484](BUG-484-OPEN.md)'s territory), then
that `getComputedStyle(div).getPropertyValue('background-image')` starts with
the gradient function name. Every `[ parsable ]` case reaches the second
assertion (the first already passes per BUG-484) and fails there — computed
value is `""`, which does not start with anything. `[ unparsable ]` cases fail
on the *first* assertion instead (BUG-484's mechanism, already tracked) and
never reach this one, so this bug's signature is specifically the
`[ parsable ]` failures:

```
FAIL linear-gradient(black, white) [ parsable ] - assert_equals: expected true but got false
FAIL repeating-radial-gradient(black 0%, green 50%, white 100%) [ parsable ] - assert_equals: expected true but got false
```

108 of 216 subtests in this file alone (all six gradient functions ×
18 `[ parsable ]` stop-list cases).

**Same gap, four more properties — confirmed by code inspection, not by their
WPT symptom text.** `grep -n '"object-fit"\|"object-position"\|"image-rendering"\|"vertical-align"'
selector_query.rs` returns nothing, while all four are genuinely parsed and
stored on `ComputedStyle` (`style.rs:3758` `object_fit`, `:3761`
`object_position`, `:3771` `image_rendering`, `:3766` `vertical_align`, all
wired through cascade/inherit) — same missing-whitelist-entry pattern as
`background-image`. **Caveat:** these four's WPT failures read
`"<prop> doesn't seem to be supported in the computed style"`, e.g.:

```
FAIL Property object-fit value 'cover' - assert_true: object-fit doesn't seem to be supported in the computed style expected true got false
FAIL Property vertical-align value 'top' - assert_true: vertical-align doesn't seem to be supported in the computed style expected true got false
```

— but that message comes from the shared helper's `property in
getComputedStyle(target)` check, which is unconditionally `false` for
*every* property, including fully-supported ones, per
[BUG-539](BUG-539-OPEN.md) (missing `Proxy` `has` trap). So this specific
symptom text is **not** independent evidence of a whitelist gap for these
four — the code-grep above is. Once BUG-539 is fixed, these four will still
need their `computed_style_to_map` entries added, or the exact same test IDs
will keep failing with a *different* message (empty-string value instead of
"not supported") — verify against `getPropertyValue`, not just `in`, when
re-triaging.

(`image-orientation`, which failed alongside these in the same log, is a
*different* class — no `image_orientation` field exists on `ComputedStyle`
at all, i.e. genuinely unimplemented rather than just unexposed; not folded
into this bug, worth its own entry if picked up.)

Any other WPT test asserting on `getComputedStyle(...).getPropertyValue(...)`
(not `in`) for a `background-*` longhand (`-position`/`-size`/`-repeat`/
`-attachment`/`-clip`/`-origin`/`-blend-mode`) or another
already-parsed-but-unlisted property will hit the same gap — not
exhaustively swept beyond what `css-images`/`css-inline` surfaced this
slice; whoever picks this up should diff `ComputedStyle`'s field list
against `computed_style_to_map`'s insert calls once, rather than fix
properties one WPT-failure at a time.

## Как исправить (не входит в объём P2)

Add the missing longhands to `computed_style_to_map`, following the existing
pattern for `background-color`/`object-fit`'s siblings that are already
there (`display`, `visibility`, etc. — plain enum-to-`&str` match arms).
`background-image` additionally needs re-serialization from `style.rs`'s
parsed gradient representation (CSS Images L3 §2.3.1 canonical order +
color-normalization — `black` → `rgb(0, 0, 0)` — via the same
`css_color_to_css`/`color_to_css` helpers already used elsewhere in this
function).
