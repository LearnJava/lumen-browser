# BUG-345: navigation to a page mixing `document.designMode` + declarative Shadow DOM hangs/times out under live BiDi

**Статус:** OPEN
**Компонент:** shell/js (unclear which layer yet — see investigation notes)
**Найден:** P2, WPT-VENDOR-contenteditable 2026-07-25 (`run_report.py --all --root contenteditable --recursive`, vendored `contenteditable/designmode-iscontenteditable.html`)

## Симптом

`run_report.py --all --root contenteditable --recursive` (live `lumen --bidi-port` +
wptrunner's `webdriver-bidi` executor) on `contenteditable/designmode-iscontenteditable.html`:

```
TEST_START: /contenteditable/designmode-iscontenteditable.html
INFO Got timeout in harness
TEST_END: TIMEOUT, expected OK - TestRunner hit external timeout (this may indicate a hang)
WARNING Exception in TestExecutor.run:
...
webdriver.bidi.error.UnknownErrorException: unknown error (navigate: automation command timed out)
```

The `browsingContext.navigate` BiDi command itself times out (not just the
testharness completion poll) — the harness never got to run any of the page's
`test()` calls.

## The page

```html
<div id="ceInherit">
  <template shadowrootmode="open">
    <span id="inShadow"></span>
  </template>
</div>
<div contenteditable="false" id="ceFalse"></div>
<script>
  document.designMode = "on";
  test(() => { assert_true(ceInherit.isContentEditable); }, "...");
  ...
</script>
```

Declarative Shadow DOM (`<template shadowrootmode="open">`) + `document.designMode`
(a plain, unimplemented property — grep confirms no `designMode` in
`crates/js/src/dom.rs`, so setting it is just an ordinary untracked JS property
write, nothing engine-specific) + 4 `test()` calls that read `isContentEditable`
through the shadow tree.

## What's been ruled out (not yet root-caused)

- **Not a whole-parse hang**: `lumen --dump-layout` (headless, one-shot, no live
  BiDi/testharness loop) completes near-instantly on both (a) an isolated
  `document.designMode = "on"` + plain div repro, and (b) an isolated
  `<template shadowrootmode="open">` repro. Also completes on the *actual*
  vendored file served from a local path (though `testharness.js` 404s there
  since it's not going through `wptserve`, so the test body itself never runs
  in that check — only rules out a parse-time infinite loop, not a
  script-execution-time one).
- Not `designMode` itself doing anything special (unimplemented, plain property).
- **Not root-caused**: whether the hang is in live incremental restyle/layout
  triggered by the shadow-DOM+contenteditable combination (same general class as
  the nested-flexbox incremental-restyle blowup being investigated under BUG-341,
  though that one is flex-specific and this page has no flex), in the shell's
  render loop, or somewhere in the BiDi navigate/DocumentReady wait path
  specifically for this kind of page, is unknown — needs the same live-window
  BiDi-eval bisection technique used for BUG-298/299/300 (`CLAUDE.md` → Known
  gotchas) or a `LUMEN_MEM_REPORT`/hang-diagnosis pass per
  `docs/automation.md`.

## Impact

A full navigation-command timeout (not a graceful per-subtest failure) is a more
severe class of gap than the usual first-pass "unsupported API" finding — a real
page combining declarative Shadow DOM with any DOM-mutating script in this shape
could make a tab appear hung to a user, in the same user-visible category as
BUG-307.

## Suggested next step

Bisect which of (a) declarative Shadow DOM being present, (b) the 4 `test()`
calls' cross-shadow-boundary property reads, or (c) something in between is
responsible, using a live `--bidi-port` session + incremental `script.evaluate`
probing (same technique as the BUG-298/299/300 investigation) rather than
`--dump-layout`, since the hang doesn't reproduce there.
