# BUG-568: `document.write()`/`.open()`/`.close()` do not exist — the whole "dynamic markup insertion" family is unimplemented

**Статус:** OPEN
**Компонент:** js (`crates/js/src/dom.rs` — `document` object literal, `dom.rs:4160` onward, has no `write`/`writeln`/`open`/`close` member at all; confirmed by `grep -n "document\.write\|\"write\"\|parseHTMLUnsafe"` returning nothing for any of the four)
**Найден:** P2, WPT-VENDOR-html-semantics-embedded-content, 2026-08-04; scope widened P2, WPT-VENDOR-html-webappapis, 2026-08-04

## Симптом

`document.write(...)`/`.writeln(...)`/`.open(...)`/`.close()` all throw
`TypeError: document.<method> is not a function` — none of the four methods
exist on the `Document` JS wrapper, not a broken stub for any of them.
Originally observed in `html/semantics/embedded-content/media-elements`
(tests that build fixture markup via `document.write` before asserting on
it):

```
FAIL getting audio.muted with muted="" (document.write-created) - document.write is not a function
FAIL getting video.muted with muted="" (document.write-created) - document.write is not a function
```

`html/webappapis/dynamic-markup-insertion/` — the module built specifically
around this API family — confirms `open`/`close` are just as absent as
`write`/`writeln`:

```
TIMEOUT html/webappapis/dynamic-markup-insertion/opening-the-input-stream/011.html
TIMEOUT html/webappapis/dynamic-markup-insertion/closing-the-input-stream/document-close-with-pending-script.html
```

## Причина

`document.write`/`document.writeln`/`document.open`/`document.close` (HTML LS
§3.6, "Dynamic markup insertion") have never been implemented on the
`Document` wrapper in `dom.rs` — there is no partial stub, no guard, just
four missing members. Any WPT test (in this or any other category) that
constructs fixture elements via `document.write('<video muted></video>')`,
or re-enters the parser via `document.open()`, fails immediately with a
`TypeError`, before reaching the assertion the test actually cares about.

## Масштаб

Low direct count in the originating category (2 distinct subtests) but
`document.write`/`.open`/`.close` are foundational, spec-required `Document`
methods with broad reach across the WPT corpus wherever fixture markup is
streamed in rather than parsed via `innerHTML`. `html/webappapis` shows the
true scale: essentially the entire
`dynamic-markup-insertion/opening-the-input-stream/` (~40 files) and
`dynamic-markup-insertion/document-write/` (~15 files) subdirectories
TIMEOUT or FAIL on this alone — the single largest failure cluster in that
slice after [BUG-591](bugs/BUG-591-OPEN.md) (global error reporting).
