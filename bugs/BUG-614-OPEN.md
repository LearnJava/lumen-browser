# BUG-614: `<ruby>`/`<rt>`/`<rb>`/`<rp>`/`<rtc>` default to `display: block`, breaking even no-ruby-support fallback rendering

**Статус:** OPEN
**Компонент:** layout (`crates/engine/layout/src/style.rs::default_display`, ~line 10484)
**Найден:** P2, WPT-VENDOR-html-ruby-extensions, 2026-08-04

## Симптом

`--dump-layout` on `tests/wpt/html-ruby-extensions/html-ruby-001.html`
(`<ruby><span>浄</span><rt>じょう</ruby>` markup):

```
Block rect=(8.00, 197.80, 1008.00, 38.40)
  InlineRun rect=(8.00, 197.80, 1008.00, 19.20)
    seg[0] "浄" color=#ffa500ff
  Block rect=(8.00, 217.00, 1008.00, 19.20) color=#ffa500ff
    InlineRun rect=(8.00, 217.00, 1008.00, 19.20) color=#ffa500ff
      seg[0] "じょう" color=#ffa500ff
```

Every ruby base and its `<rt>` annotation render as two *separate block
boxes*, each on its own line, instead of flowing as consecutive inline
content. Per the category's own `README.md` (these are reftest-only, hand-
written "mismatch" tests: no ruby support should render like the mismatch
references, i.e. base + annotation as plain sequential inline text — e.g.
"浄じょう" running inline with surrounding context), Lumen's fallback is
*worse* than "no ruby support": text that should stay in one paragraph
splits onto extra lines per base/annotation pair, corrupting reading order
for any page using `<ruby>` regardless of whether ruby-specific styling is
ever implemented.

## Причина

`default_display()` (`style.rs:10484`) has no arm for `ruby`/`rb`/`rt`/`rp`/
`rtc` — they fall through the catch-all `_ => Display::Block` at line 10531.
The dedicated ruby box machinery (`crates/engine/layout/src/ruby.rs`,
`RubyBox::from_style`, `lay_out_ruby`) exists and consumes the `ruby-*` CSS
properties, but per `CAPABILITIES.md`'s own note it "has no pipeline
callers" — nothing wires `<ruby>` elements into the box tree at all, so the
element degrades to whatever `default_display` returns for an unknown tag,
which is `Block`.

Minimal fix (restores correct *fallback* behavior without requiring the
full ruby box model): add `"ruby" | "rb" | "rt" | "rtc" | "rp"` to the
existing inline-elements arm (`style.rs:10498-10501`, alongside `del`/`ins`/
`s`) so ruby markup at least flows as normal inline content, matching what
every browser renders when ruby-specific layout is absent. Wiring the full
`RubyBox`/`lay_out_ruby` pipeline (the `ruby-position`/`ruby-align`/
`ruby-merge` visual behavior) is a separate, larger P1 task already tracked
in `CAPABILITIES.md`.

## Масштаб

Confirmed via `--dump-layout` on 1 file; the same `<ruby>`/`<rt>` pattern
recurs across all 84 automatically-selected ids in the category (all
`rel=mismatch` reftests, `run_report.py` gives 0/0 — "Unsupported test type
reftest", no automated signal). `<rp>`/`<rtc>` group syntax appears in
`html-ruby-101`+ / `html-ruby-301`+ respectively — same `default_display`
gap applies, not independently re-tested. Category otherwise gives no
automatable output; this is the run's only concrete finding.
