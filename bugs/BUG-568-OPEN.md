# BUG-568: `document.write()` does not exist

**Статус:** OPEN
**Компонент:** js (`crates/js/src/dom.rs` — `Document` object has no `write`/`writeln` member at all; confirmed by `grep -n "document\.write\|\"write\"" crates/js/src/dom.rs` returning nothing)
**Найден:** P2, WPT-VENDOR-html-semantics-embedded-content, 2026-08-04

## Симптом

`document.write(...)` throws `TypeError: document.write is not a function` — the
method is simply absent from the `Document` JS wrapper, not a broken stub.
Observed in `html/semantics/embedded-content/media-elements` (tests that build
fixture markup via `document.write` before asserting on it):

```
FAIL getting audio.muted with muted="" (document.write-created) - document.write is not a function
FAIL getting video.muted with muted="" (document.write-created) - document.write is not a function
```

## Причина

`document.write`/`document.writeln` (HTML LS §3.6, "Dynamic markup insertion")
have never been implemented on the `Document` wrapper in `dom.rs` — there is no
partial stub, no guard, just a missing member. Any WPT test (in this or any
other category) that constructs fixture elements via
`document.write('<video muted></video>')` instead of `createElement`/
`innerHTML` fails immediately with this `TypeError`, before reaching the
assertion the test actually cares about.

## Масштаб

Low direct count in this category (2 distinct subtests, 4 log lines from
duplicate harness/subtest reporting) but `document.write` is a foundational,
spec-required `Document` method with broad reach across the WPT corpus
wherever fixture markup is streamed in rather than parsed via `innerHTML` —
worth fixing for its own sake, not just this category's numbers.
