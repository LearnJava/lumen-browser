# BUG-595: `autocorrect`/`writingsuggestions` global content attributes missing entirely (not even as a property)

**Статус:** FIXED 2026-09-16 (P3)
**Компонент:** js (`crates/js/src/shim/web_api_shim_tail_b.js`, `HTMLElement.prototype` global-attributes reflection table)
**Найден:** P2, WPT-VENDOR-html-editing, 2026-08-04

## Исправление

Новый режим `'onoff-bool'` в `_lumen_define_reflection` для `autocorrect`:
геттер `true`, если атрибут не выставлен в `"off"` (case-insensitively) --
отсутствие, любое другое значение и невалидное значение все читаются как
`true`; сеттер пишет ключевое слово `"on"`/`"off"` по `ToBoolean(v)`.

`writingSuggestions` не укладывается в generic-таблицу (единственная запись
там с наследованием по дереву элементов), поэтому это отдельный
`Object.defineProperty(HTMLElement.prototype, 'writingSuggestions', ...)`:
геттер идёт по `parentElement` до первого предка с валидным собственным
значением (`"true"`/`"false"` case-insensitively, любая другая строка
невалидна), дефолт `"true"` в корне дерева; сеттер -- обычный string-reflect
(`String(v)` вербатим -- невалидная строка всё равно попадает в атрибут,
как того требует `writingsuggestions.html`'s
`testSetAttributeDirectly('foo', ...)`-кейсы). Алгоритм построчно проверен
против всех тестов вендоренного `writingsuggestions.html`.

**Остаток, не входивший в исходную заявку:** `autocorrect` спекой
наследуется от **form owner** (HTML LS §4.10.20.1), а не от DOM-предка, и
некоторые типы `<input>` (password/email/url) обязаны читать `false`
всегда -- ни то ни другое не реализовано, только собственное значение
элемента. Это отдельный, более глубокий алгоритм; исходная заявка о нём не
упоминала (говорила только об отсутствии самого свойства), отдельный
GAP-тикет не заводился.

9 новых тестов в `crates/js/src/dom/tests/v8_details_dialog_popover.rs`
(`autocorrect_*`/`writing_suggestions_*`). `cargo test -p lumen-js
--profile dev-release --features v8-backend v8_details_dialog_popover`
зелёный (95/95), `cargo clippy -p lumen-js --all-targets --features
v8-backend -- -D warnings` чист.

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
