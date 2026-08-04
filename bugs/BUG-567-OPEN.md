# BUG-567: `HTMLTitleElement.prototype.text` does not exist

**Статус:** OPEN
**Компонент:** js (`crates/js/src/dom.rs` — `HTMLTitleElement` is registered
as an interface/tag-constructor name (`dom.rs:4614`, `dom.rs:4648`: `'TITLE':
HTMLTitleElement`) but nothing ever defines a `.text` getter/setter on its
prototype or instances; contrast `document.title`, which does have a real
`get`/`set` pair at `dom.rs:7152-7153`)
**Найден:** P2, WPT-VENDOR-html-semantics-document-metadata, 2026-08-04

## Симптом

`document.querySelector('title').text` (equivalently
`document.getElementsByTagName("title")[0].text`) is `undefined` — both
reading and writing. Confirmed by direct source read
(`grep -n "HTMLTitleElement" crates/js/src/dom.rs` returns only the two tag
registration lines above, zero property definitions) and by every subtest of
`the-title-element/title.text-01.html` and `title.text-03.html`:

```
FAIL COMMENT - assert_equals: expected (string) "TEXT" but got (undefined) undefined
FAIL title.text and space normalization (markup) - assert_equals: expected (string) " title.text  and space normalization  " but got (undefined) undefined
FAIL title.text and space normalization: "one space" - assert_equals: expected (string) "one space" but got (undefined) undefined
… (22 subtests total across the two files, all the same shape)
```

## Причина

Per HTML LS §4.2.2, `HTMLTitleElement.text` is a distinct IDL attribute from
`Node.textContent`: the getter concatenates the `Text` node data of all
*direct child* `Text` nodes (ignoring comments/elements, unlike
`textContent` which walks the whole subtree), and the setter replaces all
children with a single `Text` node holding the given value — exactly the
same "child-text concatenation vs. subtree concatenation" contract that
distinguishes them (`title.text-01.html`'s own assertion pins this: `title.
text === "TEXT"` while `title.textContent === "TEXTELEMENT"` after mixing a
`<a>ELEMENT</a>` child in). Nobody has added it — the property is a bare gap,
not a deliberate stub, on an interface (`HTMLTitleElement`) that otherwise
only exists as a tag-name→constructor mapping entry with no members of its
own.

## Масштаб

22 subtests across `title.text-01.html` (comment/element-child mixing) and
`title.text-03.html` (whitespace/control-character normalization: tab,
newline, form feed, CR, CRLF-family sequences, doubled variants) — every one
fails identically on `undefined`. `title-multiple-elements.html` and
`title.text-02.html`(if vendored) were not reached with clean signal in this
run (timed out on an unrelated `module 'foo' not found` error before
reaching the relevant assertions) and should be re-checked once this is
fixed.
