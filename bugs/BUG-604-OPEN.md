# BUG-604: no UA (user-agent) shadow tree for `<video>`/`<audio>`/`<select>`/`<details>` — light-DOM children render directly instead of being hidden/slotted per spec

**Статус:** OPEN
**Компонент:** layout/dom (no internal shadow-root construction anywhere for these interfaces; the fix is architectural, not a one-line gap)
**Найден:** P2, WPT-VENDOR-html-rendering, 2026-08-04

## Симптом

```
FAIL <video></video> has a shadow tree with no slots - child.getClientRects is not a function
FAIL <select></select> has a shadow tree with slot - assert_not_equals: child should be in the flat tree got disallowed value 0
```
(`widgets/shadow-dom.html` — the `getClientRects` `TypeError`s are
[BUG-478](BUG-478-OPEN.md)/[BUG-522](BUG-522-OPEN.md)/[BUG-551](BUG-551-DUPLICATE.md)/[BUG-580](BUG-580-DUPLICATE.md)
territory and the `outerHTML`-in-test-name-collision harness `ERROR` is
[BUG-351](BUG-351-OPEN.md); this bug is the *assertion content* underneath
both, once those are stripped out)

## Причина

HTML LS §4.8.11(video/audio)/§4.10.11(select)/§4.11.1(details) specify each
of these elements ships with an internal ("UA") shadow tree: `<video>`/
`<audio>` render their built-in controls through one with **no slot at
all**, so any light-DOM child a page appends is never part of the flat tree
— `getClientRects().length` must be `0` and `getComputedStyle(child).length`
must be `0` (element outside the rendered tree). `<select>`/`<details>`
instead have a shadow tree **with** a slot, so an appended child *is*
rendered but inherits from the slot per the UA stylesheet
(`display: contents` etc.), not from ordinary cascade rules.

Lumen has no internal shadow-root construction for any of these four
interfaces — a `<span>` appended to a live `<video>` becomes an ordinary
rendered light-DOM child (violates the "no slot" contract), and one
appended to `<select>`/`<details>` does not go through the expected
UA-stylesheet-driven slot inheritance path either. This is a gap in the
element implementations themselves, not in the generic Shadow DOM machinery
(author-created `attachShadow` shadow roots work correctly elsewhere in the
corpus).

## Масштаб

Architectural — needs an actual internal shadow root per interface, wired
into each element's construction, not a display-property tweak. Confirmed
narrowly (4 elements, 1 file, 9 subtests) in this slice; likely affects any
other WPT test that assumes UA shadow tree encapsulation for these same
four elements elsewhere in the vendored corpus (not swept beyond this file).
