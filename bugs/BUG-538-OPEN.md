# BUG-538: `text-box-trim`/`text-box-edge` (CSS Inline L3) not implemented at all

**Статус:** OPEN
**Дата:** 2026-08-03
**Компонент:** css-parser + layout (no `ComputedStyle` field exists yet)
**Найден:** WPT-RUN-3 срез 28 (`ROADMAP.md`) — массовый прогон `css/css-inline`

## Механизм

Neither `text_box_trim` nor `text_box_edge` exists as a `ComputedStyle` field
(`grep -n 'text_box_trim\|text_box_edge' crates/engine/layout/src/style.rs`
returns nothing) — this is a genuinely unimplemented CSS Inline Layout L3
feature, not a serialization gap like [BUG-537](BUG-537-OPEN.md). Not
tracked in `CSS-SPECS.md` either (both names absent), so the module itself
was never queued for P4.

`text-box-trim` shorthand's parent, `text-box` (the `text-box-trim ||
text-box-edge` shorthand), is also unimplemented by the same absence.

## Симптом

Every inline-style assignment to either longhand is accepted verbatim and
echoed back by the generic (non-validating) style setter —
[BUG-484](BUG-484-OPEN.md)'s mechanism, not specific to this bug — while
every `getComputedStyle()` probe reports "doesn't seem to be supported":

```
FAIL Property text-box-trim value 'trim-start' - assert_true: text-box-trim doesn't seem to be supported in the computed style expected true got false
FAIL Property text-box-edge value 'text ideographic-ink' - assert_true: text-box-edge doesn't seem to be supported in the computed style expected true got false
```

(That exact message is also produced unconditionally by [BUG-539](BUG-539-OPEN.md)
for *any* property, implemented or not — but unlike the BUG-537 cases, these
two genuinely have no `ComputedStyle` field to serialize in the first place,
so this bug stands independently of BUG-539.)

`text-box-trim/*.html` + `text-box-edge/*.html` account for the bulk of
`css/css-inline`'s 512/640 failing subtests this slice (213 log lines
mention either property name). `text-box-trim-om-001.html` additionally
TIMEOUTs outright (OM/transition test, needs the property to exist before
its animation behavior can be probed at all).

## Как исправить (не входит в объём P2)

P4 track: add `text_box_trim: TextBoxTrim` / `text_box_edge: TextBoxEdge`
fields to `ComputedStyle`, parse per CSS Inline L3 §2 grammar
(`trim-start | trim-end | trim-both | none` / `text-edge-style{1,2}` with
the `auto | text | cap | ex | alphabetic | ideographic | ideographic-ink`
vocabulary + the `leading` fallback keyword), wire cascade/inherit (both
non-inherited per spec), then add both to `computed_style_to_map` so this
doesn't immediately re-trigger BUG-537's class once implemented.
