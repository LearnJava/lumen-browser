# BUG-468: percentage margin/padding not re-resolved after JS `style.width` mutation on the containing block

**Статус:** DUPLICATE → [BUG-493](BUG-493-OPEN.md)
**Дата:** 2026-08-02
**Найден:** WPT-RUN-3 срез 2 (`ROADMAP.md`) — массовый прогон `css/CSS2`, 8
files `css/CSS2/normal-flow/containing-block-percent-{padding,margin}-{left,right,top,bottom}.html`
(1/1 сабтест FAIL in each)

## Дубликат установлен P3 2026-09-01

Probed live (`--dump-layout` with `console.log` bracketing the mutation) to
confirm the exact mechanism before attempting a fix — see
`docs/probe-method.md`. Confirmed the bug description's own diagnosis was
imprecise ("измеряется 0") and the true root is the same architectural gap
already tracked as [BUG-493](BUG-493-OPEN.md)/[CSSOM-4](../ROADMAP.md): no
synchronous style/layout flush before a JS geometry read, so a node mutated
and read back in the *same* synchronous script tick observes the pre-mutation
snapshot, not zero and not a fresh recompute.

```
<div id="container" style="width:123px;">
  <div id="child"></div>  <!-- CSS: padding-left:10%; width:50px; height:100px -->
</div>
<script>
  console.log(child.offsetWidth);              // 62.3 (correct: 50 + 12.3 = 10% of 123px)
  container.style.width = "500px";
  console.log(child.offsetWidth);               // STILL 62.3 — stale, not recomputed
</script>
```

Generality confirmed with a second, percent-free probe (`.tmp/bug468-repro3.html`
in the investigating session — not committed): a plain `<div style="width:50px">`
mutated to `width:300px` and read back in the same tick also reports the
stale `50`, not `300`. This rules out anything specific to percentage
resolution against a containing block — the defect is generic to *any*
layout-dependent JS read after a same-tick mutation, exactly BUG-493's
finding (there confirmed for `getComputedStyle()`; BUG-493's срез 12 already
extended it to `offsetWidth`/`clientWidth`).

**Why BUG-493 survives instead of this bug**, despite this bug carrying the
lower number (WPT-RUN-3 срез 2, filed before BUG-493's срез 8 in the same
run — by the letter of `BUGS.md`'s "earliest survives" convention this bug
would normally be the anchor): by the time the duplicate was recognised,
BUG-493 already carried 15 documented investigation slices, was integrated
into `ROADMAP.md` as [CSSOM-4](../ROADMAP.md), and is narratively
cross-referenced from six sibling bugs (BUG-472/494/499/503/523/530) and
`docs/wpt-vendor-notes/css.md`. Renaming the anchor now would be a purely
cosmetic, high-risk rewrite across that whole body of prose for zero
functional benefit. This bug's own unique contribution — the 8
`css/CSS2/normal-flow/containing-block-percent-*` files, plus the
absolute-px confirmation above — has been appended to BUG-493 as a new
slice instead.

## Симптом (original filing, kept for the record)

All 8 files use the same pattern:

```html
<div id="container" style="width:123px;">
  <div data-expected-width="100" data-expected-height="100"></div>
</div>
<script src="/resources/check-layout-th.js"></script>
<script>
  document.body.offsetTop;                                     // forces initial layout
  document.getElementById("container").style.width = "500px";  // mutate container
  checkLayout("#container");                                    // immediate check
</script>
```

The child sets `padding-left:10%`/`margin-top:50%`/etc. (percentages resolve
against the containing block width, CSS2.1 §10.2/§8.3). `checkLayout` expects
the percentage offset to be recomputed against the new width; instead it
reads back the pre-mutation snapshot.

## .ini

`tests/wpt/metadata/css/CSS2/normal-flow/containing-block-percent-{padding-left,padding-right,padding-top,padding-bottom,margin-left,margin-right,margin-top,margin-bottom}.html.ini`
— one `expected: FAIL` subtest each. Left untouched; already correctly
`FAIL`-annotated and, per BUG-493, will keep failing until CSSOM-4 lands.
