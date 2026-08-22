# BUG-557: live global `document` object has `appendChild` but no `removeChild`/`insertBefore`/`replaceChild`

**Статус:** OPEN
**Дата:** 2026-08-04
**Компонент:** js (`crates/js/src/dom.rs:7125` — the `var document = {…}` literal behind the live global `document`)
**Найден:** P2, WPT-RUN-3 срез 39 (`css/cssom-view`), 2026-08-04

## Симптом

```
FAIL CSSOM View - 7 - element.offsetWidth detatches correctly
  document.removeChild is not a function
```

`htmlelement-offset-width-001.html` calls `document.removeChild
(document.documentElement)` on the live global document and gets a
`TypeError` instead of the node being detached (or, per spec, `NotFoundError`
if it were somehow not a child).

## Причина

The live global `document` literal (`dom.rs:7125`, ~276 lines) defines
`appendChild` (`dom.rs:7277`, inside the literal) but has no
`removeChild`/`insertBefore`/`replaceChild` at all — `grep -n
"removeChild\|insertBefore\|replaceChild" crates/js/src/dom.rs` finds these
only on: the ordinary element wrapper (`_lumen_build_element`,
`dom.rs:5826`), `DocumentFragment` (`dom.rs:4396`), and `CharacterData`
(`dom.rs:4480`, throws by design). The live `document` object was apparently
never given the rest of the `Node` mutation interface, only the one method
(`appendChild`) that happened to be needed elsewhere.

Same subsystem, same "two independently-written document objects with
non-overlapping holes" pattern already flagged by
[BUG-358](bugs/BUG-358-OPEN.md) (live document missing metadata attributes:
`characterSet`/`URL`/`compatMode`/…) and
[BUG-415](bugs/BUG-415-FIXED.md) (the *detached* document from
`createHTMLDocument`/`createDocument` missing the same `Node` methods, plus
HTML accessors) — this is the third, distinct hole in the same pair of
objects: the **live** document additionally lacks `removeChild`/
`insertBefore`/`replaceChild` specifically (as opposed to BUG-358's
metadata-attribute gap). Worth fixing together with BUG-358/BUG-415 by one
shared document-object builder, per BUG-415's own recommendation.

## Масштаб находки

1 file / 1 subtest this slice (`htmlelement-offset-width-001.html`), but
`document.removeChild`/`insertBefore`/`replaceChild` on the live document is
a basic enough API that any WPT test using it to reset document state
between assertions will hit the same wall.

## Что нужно

Give the live `document` literal the same `removeChild`/`insertBefore`/
`replaceChild` implementations the ordinary element wrapper already has
(`dom.rs:5826` area) — ideally as part of the shared builder BUG-415
proposes rather than a fourth independent copy.
