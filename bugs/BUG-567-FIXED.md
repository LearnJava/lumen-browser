# BUG-567: `HTMLTitleElement.prototype.text` does not exist

**Статус:** FIXED 2026-09-14
**Компонент:** js (`crates/js/src/shim/web_api_shim_mid.js` — `dom.rs` с тех
пор расщеплён на `crates/js/src/dom/*`; `HTMLTitleElement` регистрируется как
интерфейс/тег-конструктор в общем цикле генерации HTML-интерфейсов, но
ничего не определяло `.text` на его прототипе; contrast `document.title`,
у которого уже был настоящий `get`/`set`)
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

## Исправление

`HTMLTitleElement.prototype.text` определён в `web_api_shim_mid.js` сразу
после генерации HTML-интерфейсов: геттер суммирует `data` только прямых
дочерних узлов с `nodeType === 3` (используя уже существующие `childNodes`/
`nodeType`/`data`, без новых нативных привязок), сеттер делегирует в
`textContent` — ровно алгоритм спеки. Регрессия —
`crates/js/src/dom/tests/v8_bug567_title_text.rs` (3 теста: обычный текстовый
child, смесь комментарий+текст+вложенный элемент — дословная транскрипция
`title.text-01.html`, сеттер не нормализует пробелы — транскрипция
`title.text-03.html`). `cargo test -p lumen-js --features v8-backend`:
3640/3640 (было 3637). `cargo clippy -p lumen-js --all-targets --features
v8-backend -- -D warnings` чист.
