# BUG-595: `autocorrect`/`writingsuggestions` global content attributes missing entirely (not even as a property)

**Статус:** OPEN
**Компонент:** js (`crates/js/src/dom.rs:10724-10735`, `HTMLElement.prototype` global-attributes reflection table)
**Найден:** P2, WPT-VENDOR-html-editing, 2026-08-04

## Симптом

```
FAIL Test that the autocorrect attribute is available on HTMLInputElement. - assert_true: expected true got false
FAIL Test that the autocorrect attribute is available on HTMLTextAreaElement. - assert_true: expected true got false
FAIL Test that the autocorrect attribute is available on div. - assert_true: expected true got false
FAIL Test that the autocorrect attribute is available on form. - assert_true: expected true got false
FAIL Test that the writingsuggestions attribute is available on HTMLInputElement. - assert_true: expected true got false
FAIL Test that the writingsuggestions attribute is available on HTMLTextAreaElement. - assert_true: expected true got false
FAIL Test that the writingsuggestions attribute is available on HTMLDivElement. - assert_true: expected true got false
FAIL Test that the writingsuggestions attribute is available on HTMLSpanElement. - assert_true: expected true got false
FAIL Test that the writingsuggestions attribute is available on custom elements. - assert_true: expected true got false
```
Plus 9 `Cannot read properties of undefined (reading 'writingSuggestions')`
and a run of `assert_equals: expected (string) "true"/"false" but got
(undefined) undefined` in
`the-writingsuggestions-attribute/writingsuggestions-inheritance.tentative.html`
and `autocorrect/*`.

## Причина

`_lumen_install_reflection(HTMLElement.prototype, [...])` (`dom.rs:10724`)
lists `title`/`lang`/`dir`/`hidden`/`inert`/`accessKey`/`autocapitalize`/
`enterKeyHint`/`inputMode`/`nonce` as HTML LS §3.2.6 global attributes, but
omits `autocorrect` and `writingSuggestions` entirely -- `grep -rn
"autocorrect\|writingSuggestions\|writingsuggestions" crates/js/src/dom.rs`
is a zero-hit. Both are real HTML LS global content attributes (`autocorrect`
reflects as boolean-like `"on"/"off"` string; `writingSuggestions` reflects
as a tristate similar to `contenteditable`, inherited from the nearest
ancestor that sets it explicitly when the element's own value is `"inherit"`
or absent). Because the reflection table is the only place these IDL
attributes could be installed, `'autocorrect' in element` and
`'writingSuggestions' in element` are both `false` on every element,
including `<input>`/`<textarea>` where user-agents most commonly expose them.

## Масштаб

Two full test files (`autocorrect/*`, `the-writingsuggestions-attribute/*`)
plus an inheritance-focused `.tentative.html` -- all of their subtests fail
on the same "attribute is available" precondition before reaching any
inheritance-specific assertion, so the inheritance algorithm itself remains
completely unverified until this lands.
