# BUG-344: `contentEditable`/`isContentEditable` don't recognize the `plaintext-only` value

**Статус:** OPEN
**Компонент:** js (`crates/js/src/dom.rs`, `contentEditable`/`isContentEditable` IDL, `_lumen_is_contenteditable`)
**Найден:** P2, WPT-VENDOR-contenteditable 2026-07-25 (`run_report.py --all --root contenteditable --recursive`, vendored `contenteditable/plaintext-only.html`)

## Симптом

WPT test `plaintext-only.html`:

```js
test(() => {
  var div = document.createElement("div");
  div.setAttribute("contenteditable", "plaintext-only");
  assert_equals(div.contentEditable, "plaintext-only");
}, "plaintext-only is an accepted attribute value for contenteditable");

test(() => {
  var div = document.createElement("div");
  div.contentEditable = "plaintext-only";
  assert_true(div.isContentEditable);
}, "plaintext-only can be assigned to contenteditable dynamically");
```

Both fail: the getter returns `"inherit"` instead of `"plaintext-only"`, and
`isContentEditable` is `false` after assigning `"plaintext-only"`.

## Root cause

The HTML spec (`contenteditable` attribute, HTML LS §6.11.3) defines **three** states:
`true`, `plaintext-only`, and `false`/`inherit`. Lumen's implementation only knows
two:

- `contentEditable` setter (`crates/js/src/dom.rs:6344`-`6346`) maps `"true"`/`"false"`
  to the attribute and treats anything else (including `"plaintext-only"`) as "remove
  the attribute" — silently downgrading `plaintext-only` to `inherit`.
- `_lumen_is_contenteditable` (`crates/js/src/dom.rs:2769`-`2774`, native binding) only
  checks for a "truthy" `contenteditable` value on the node or an ancestor — doesn't
  special-case `plaintext-only` as also making `isContentEditable` true.

## Impact

WPT-only for now (this attribute value isn't exercised elsewhere in the vendored
corpus yet), but `plaintext-only` is a real, standardized editing mode (paste-as-text,
no rich formatting) used by some web editors — silently coercing it to `inherit`
means such content becomes non-editable instead of plaintext-editable.

## Suspected fix direction

Extend the `contentEditable` setter to also accept/reflect `"plaintext-only"` verbatim
(don't fold it into the true/false/remove three-way), and extend
`_lumen_is_contenteditable`'s truthy check to treat `plaintext-only` the same as `true`
for the purposes of `isContentEditable`. The actual plaintext-only *editing behavior*
(no rich markup on input) is separate follow-up work — this bug is scoped to the
attribute/IDL reflection gap the WPT test caught.
