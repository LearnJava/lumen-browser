# BUG-600: HTML "focus fixup rule" not implemented — focus stays on an element after it becomes non-focusable (disabled/hidden/detached/loses tabindex)

**Статус:** OPEN
**Компонент:** js/shell focus pipeline (`crates/js/src/dom.rs` focus install path, or wherever `focused_node` reacts to attribute/DOM mutations — see BUG-381's fixed focus API for the surrounding machinery)
**Найден:** P2, WPT-VENDOR-html-interaction, 2026-08-04

## Симптом

```
FAIL #button1 - assert_not_equals: focus is fixed up got disallowed value object "[object Object]"
FAIL #button2 - assert_not_equals: focus is fixed up got disallowed value object "[object Object]"
FAIL #button5 - assert_not_equals: focus is fixed up got disallowed value object "[object Object]"
```
(`processing-model/focus-fixup-rule-one-no-dialogs.html`, 6/8 subtests —
the other 2 use a `ResizeObserver`+two-`requestAnimationFrame` timing check
that never resolves for an unrelated reason and don't reach this assertion)

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
