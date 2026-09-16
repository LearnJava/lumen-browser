# BUG-600: HTML "focus fixup rule" not implemented — focus stays on an element after it becomes non-focusable (disabled/hidden/detached/loses tabindex)

**Статус:** FIXED 2026-09-16 (P3)
**Компонент:** js (`crates/js/src/shim/web_api_shim_tail_b.js`, `_lumen_is_focusable`/new `_lumen_focus_fixup`; `web_api_shim_mid_b.js`, `_lumen_run_raf_callbacks`)
**Найден:** P2, WPT-VENDOR-html-interaction, 2026-08-04

## Симптом

```
FAIL #button1 - assert_not_equals: focus is fixed up got disallowed value object "[object Object]"
FAIL #button2 - assert_not_equals: focus is fixed up got disallowed value object "[object Object]"
FAIL #button5 - assert_not_equals: focus is fixed up got disallowed value object "[object Object]"
```
(`processing-model/focus-fixup-rule-one-no-dialogs.html`, 6/8 subtests —
the other 2, `.remove()` and `visibility: hidden`, are a different,
synchronous assertion path not covered by this bug)

## Причина

Per the HTML spec's [focus fixup
rule](https://html.spec.whatwg.org/multipage/interaction.html#focus-fixup-rule),
whenever the currently-focused element stops being a *focusable area* (gets
`disabled`, `hidden`, removed from the document, loses its enclosing
`<fieldset>`'s disabled state exemption, loses `tabindex`, or has
`contentEditable` turned off), the UA must run the fixup algorithm at the
next "update the rendering" step and move focus to `document.body` (absent
an ancestor `<dialog>`/popover taking it over).

Each `test_focus_fixup(selector, change)` case in the vendored test:
1. Focuses `el` and asserts `document.activeElement === el` (passes —
   BUG-381's focus API works for the initial focus).
2. Runs `change(el)` (e.g. `button.disabled = true`).
3. Waits one `requestAnimationFrame` + `ResizeObserver` cycle.
4. Asserts `document.activeElement !== el` and `document.activeElement ===
   document.body`.

Step 4 fails: Lumen leaves `document.activeElement` pointing at the
now-non-focusable element instead of moving it to `document.body` — the
fixup algorithm itself doesn't exist, only the initial-focus path does.

## Масштаб

6 of 8 subtests in the one vendored file covering this rule (`disabled`,
`hidden`, `fieldset disabled`, `legend re-inserted into a disabled fieldset`,
losing `tabindex`, `contentEditable` turned off — the `.remove()` and
`visibility: hidden` cases use a different, synchronous assertion path not
covered by this bug). Any page script relying on `document.activeElement`
staying in sync with actual focusability (e.g. a form library disabling the
currently-focused submit button and expecting focus to move on) is affected
silently, not just this WPT file.

## Исправлено

New `_lumen_focus_fixup()` (`web_api_shim_tail_b.js`, next to
`_lumen_focus_update`): if `_lumen_last_focused_nid` is set and
`_lumen_is_focusable` now says no, refocuses to `document.body` (via
`_lumen_focus_update(-1)`, which `activeElement`'s existing no-focus fallback
already resolves to body — no dialog/popover-aware target selection needed,
none of this bug's subtests nest one). Called from
`_lumen_run_raf_callbacks` (`web_api_shim_mid_b.js`) — the shim's per-frame
stand-in for "update the rendering" — *after* the rAF callback batch runs,
matching the spec's "fixup runs at the end of the step" ordering (a callback
reading `document.activeElement` must still see the pre-fixup value).

Two focusability gaps in `_lumen_is_focusable` needed closing for the fixup
to actually see these elements go non-focusable:
- `hidden` was never checked at all (rolled into the existing inert-ancestor
  walk, since `hidden` hides the whole subtree the same way `inert` does).
- disablement via an ancestor `<fieldset disabled>` (not the element's own
  `disabled` attribute) wasn't checked. Rather than re-implement the
  fieldset/first-`<legend>`-child exemption in JS, this calls
  `_lumen_node_matches_selector(nid, ':disabled')`, reusing the Rust
  `:disabled` selector matcher (`crates/engine/layout/src/style/matching/
  forms.rs::is_actually_disabled`) that already encodes that exact rule for
  CSS matching.

New test `v8_runtime::tests::dom_suspend_focus::
focus_fixup_rule_moves_focus_to_body` drives all 6 in-scope scenarios
directly against the engine (focus → mutate → assert not-yet-fixed-up →
`_lumen_run_raf_callbacks(0)` → assert fixed up to body) rather than through
the WPT harness, because the vendored file's own async assertions turned out
to depend on `ResizeObserver`-vs-`requestAnimationFrame` ordering that this
engine doesn't guarantee — a separate, pre-existing scheduling gap, filed as
[BUG-1056](BUG-1056-OPEN.md) rather than folded into this fix. `cargo test -p
lumen-js --features v8-backend focus_fixup` and `cargo clippy -p lumen-js
--all-targets --features v8-backend -- -D warnings` are both green.
