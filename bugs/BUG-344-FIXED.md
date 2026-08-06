# BUG-344: `contentEditable`/`isContentEditable` don't recognize the `plaintext-only` value

**Статус:** FIXED 2026-08-06
**Компонент:** js (`crates/js/src/dom.rs`, `contentEditable`/`isContentEditable` IDL, `_lumen_is_contenteditable`)
**Найден:** P2, WPT-VENDOR-contenteditable 2026-07-25 (`run_report.py --all --root contenteditable --recursive`, vendored `contenteditable/plaintext-only.html`)
**Исправлено:** P3 2026-08-06

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

## Fix (P3, 2026-08-06)

The suspected root cause was half right: only the `contentEditable` getter/setter
(JS shim, `crates/js/src/dom.rs:3527`-`3546`) needed a change. `_lumen_is_contenteditable`
(native, `v8_runtime.rs`) delegates to `lumen_dom::find_editing_host` →
`node_is_contenteditable` (`crates/engine/dom/src/lib.rs:1942`), which already treats
**any** `contenteditable` attribute value other than `"false"` as truthy — so once the
attribute actually holds `"plaintext-only"` instead of being stripped, `isContentEditable`
was already correct with no native-side change.

The real bug was entirely in the setter: any value other than `"true"`/`"false"`
(including `"plaintext-only"`) fell into the `else` branch and called
`_lumen_remove_attr`, silently downgrading the state to `inherit` — which is why
`isContentEditable` read back `false` after assigning `"plaintext-only"`, not because
the native truthy check rejected the value.

**Change:** `contentEditable` getter now recognizes `v.toLowerCase() === 'plaintext-only'`
and returns it verbatim (instead of falling through to `'inherit'`); the setter now
special-cases `s === 'plaintext-only'` and writes the attribute instead of removing it.
Two regression tests added next to the existing contenteditable coverage in `dom.rs`
(`contenteditable_property_plaintext_only_attribute`, `contenteditable_set_property_plaintext_only`).
`cargo test -p lumen-js --features v8-backend`: 2494 passed, 0 failed — no regressions.
Plaintext-only *editing behavior* (no rich markup on input) remains separate follow-up
work, out of scope for this attribute/IDL reflection fix.
