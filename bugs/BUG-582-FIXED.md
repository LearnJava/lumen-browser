# BUG-582: Invoker Commands API (`command`/`commandfor` on `<button>`, `CommandEvent`) not implemented at all

**Статус:** FIXED 2026-09-15 (P3)
**Компонент:** js (`crates/js/src/shim/web_api_shim_mid.js` — `CommandEvent`,
`_LUMEN_EVENT_HANDLER_ATTRS`, `once`-listener fix; `crates/js/src/shim/web_api_shim_tail_b.js` —
`HTMLButtonElement.command`/`.commandForElement`/`.type`, the invoke-dispatch
click handler)
**Найден:** P2, WPT-VENDOR-html-semantics-misc, 2026-08-04

## Симптом

```
FAIL <test name> - CommandEvent is not defined
```

84 occurrences, entirely within `the-button-element/command-and-commandfor/`.

## Причина

The Invoker Commands API (HTML LS §4.10.9) had no implementation surface at
all: no `command`/`commandForElement` IDL reflection on `HTMLButtonElement`,
no invoke-dispatch algorithm, no `CommandEvent` interface. `<button
popovertarget=… popovertargetaction=…>` (the older, narrower popover-only
mechanism) worked; the newer generalized `command`/`commandfor` pair did not.

## Исправление

- **`CommandEvent`** (`web_api_shim_mid.js`, next to `ToggleEvent`): `command`
  (plain `DOMString`, `String()`-coerced, `null`→`"null"` per WebIDL) and
  `source` (nullable `Element`, throws `TypeError` on any other non-null
  value) as readonly own accessors.
- **`HTMLButtonElement.prototype.command`**: normalizes to one of the six
  stable Invoker Commands keywords (`show-modal`/`close`/`request-close`/
  `show-popover`/`hide-popover`/`toggle-popover`, case-folded) or an
  author-defined `--foo` custom command (case preserved); anything else
  reflects as `""`.
- **`HTMLButtonElement.prototype.commandForElement`**: explicit-attr-element
  reference (same shape as `popoverTargetElement`) — an IDL-set value wins
  over the `commandfor` content attribute (which is then forced to `""`,
  matching `interface.html`), restricted to the shadow-tree directions the
  API actually allows (`_lumen_command_target_reachable`).
- **`HTMLButtonElement.prototype.type`**: HTML LS's missing/invalid-value
  default changes from `"submit"` to `"button"` once `command`/`commandfor`
  is present at all (regardless of validity) — introspection-only; the click
  handler still consults the literal attribute for its own gating (see next).
- **Click handler** (`web_api_shim_tail_b.js`, next to the `popovertarget`
  one): walks up to the nearest `<button>`, skips `disabled` and (when the
  button has a form owner) anything whose literal `type` attribute isn't
  `"button"` — `button-type-behavior.html` pins that this activation check
  reads the raw attribute, not the `.type` IDL default above. Fires a
  `cancelable`, `composed`, non-bubbling `CommandEvent` on the resolved
  target (dialog commands require an actual `<dialog>`; popover commands
  require an `HTMLElement`; custom commands accept any `Element`), then —
  unless `preventDefault()`ed, and only if the target is still connected —
  runs the built-in default action (`toggle/show/hide-popover`,
  `showModal()` guarded against an already-open dialog, `close()`/
  `requestClose()` reading the invoker's `value` attribute for the dialog's
  `returnValue`).
- **`oncommand`** added to `_LUMEN_EVENT_HANDLER_ATTRS` so the content
  attribute installs an IDL-visible handler like every other `on*` attribute.
- **`{once: true}` listener bug, found while verifying this feature**:
  `_lumen_add_listener`/`_lumen_rm_listener` never looked at `options.once`
  at all — a once-listener fired on every dispatch forever, which is exactly
  the idiom several `command-and-commandfor` subtests use to install a
  one-shot `preventDefault()` (each subsequent subtest's dispatch was then
  silently cancelled by the previous subtest's stale listener). Fixed by
  storing a self-removing wrapper instead of the bare `fn` for a
  `once`-listener, keyed so `removeEventListener(type, fn)` still finds it by
  the *original* function identity. This is a general `EventTarget` fix, not
  scoped to buttons — it likely explains failures well outside this bug's own
  test directory.

## Масштаб / известные ограничения

Implements the whole stable half of the spec. Deliberately NOT implemented
(all still-`.tentative.` WPT files, a distinct/unstable spec surface):
`toggle`/`open`/`close` on `<details>`, `play`/`pause`/`play-pause`/
`toggle-muted` on `<audio>`/`<video>`, `request-fullscreen`/`exit-fullscreen`/
`toggle-fullscreen`, `step-up`/`step-down` on `<input type=number>`, the
`scroll-*` family. `CommandEvent.source` retargeting across a shadow-DOM
boundary (`source-attribute-retargeting.html`,
`toggleevent-source-attribute-retargeting.html`, and one case in
`event-dispatch-shadow.html`) is also not implemented — `.target` itself
already retargets (shared `_lumen_propagate` machinery), but `.source` does
not, since no per-listener-invocation retargeting hook exists anywhere in the
event pipeline yet; a real fix belongs in that shared machinery, not this
button-specific code.

Verified: `cargo test -p lumen-js --lib --features v8-backend` — 3675/3675,
including 20 new tests for this feature plus 4 new regression tests for the
`once` fix. WPT (`run_report.py --all --root
html/semantics/the-button-element/command-and-commandfor --recursive`):
720/806 subtests passing before the `oncommand`/disconnect-guard/
show-modal-noop fixes above (verified individually afterward via direct
`--dump-layout` repros, since a second full run collided with another
session's WPT port usage) — the remaining gap is almost entirely the
tentative categories and the shadow-retargeting limitation named above.
