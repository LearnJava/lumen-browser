# BUG-579: `HTMLDialogElement.prototype.requestClose()` missing entirely

**Статус:** FIXED 2026-09-15 (P3)
**Компонент:** js (`crates/js/src/shim/web_api_shim_mid.js` — the `<dialog>`
API block; `crates/js/src/shim/web_api_shim_tail_b.js` — modal-dialog helpers)
**Найден:** P2, WPT-VENDOR-html-semantics-misc, 2026-08-04

## Симптом

```
FAIL <test name> - dialog.requestClose is not a function
```

154 occurrences, concentrated in `interactive-elements/the-dialog-element/`.

## Причина

`requestClose([returnValue])` (HTML LS §4.11.7, closely mirroring
`CloseWatcher.requestClose()` — see the unrelated but similarly-named
[BUG-340](BUG-340-FIXED.md)) fires a cancelable `cancel` event first and
only proceeds to the normal close steps — set `returnValue`, remove `open`,
fire `close` — if `cancel` isn't prevented. It is the scriptable equivalent
of what the existing Escape-key handler already does
(`dom.rs:14939` area, `_lumen_modal_dialog_nids`-driven). The `<dialog>`
object literal (`dom.rs:5667` onward) defines `show`/`showModal`/`close`/
`returnValue` but has no `requestClose` property at all — grep for
`requestClose` in `dom.rs` only matches the unrelated `CloseWatcher` method.

## Масштаб

Large within its own feature area: every hit is inside
`the-dialog-element/`, a single subdirectory. The underlying
`cancel`→(maybe)`close` sequence and the `_lumen_modal_dialog_nids` stack
management it would need already exist and are exercised by the Escape-key
path, so this is additive (new method delegating into the same close
machinery `close()` already uses at `dom.rs:5685-5701`), not a new
subsystem.

## Исправление

The shared "close the dialog" steps (set `returnValue`, remove `open`/
`data-lumen-modal`, pop the modal stack, restore previously-focused element,
fire `close`) were factored out of `close()`'s body into a new function
`_lumen_dialog_close_steps(wrapper, nid, rv)` in `web_api_shim_tail_b.js`,
next to the Escape-key handler that already implements the same
cancel→(maybe)close sequence. `requestClose([returnValue])` dispatches a
cancelable `cancel` event and, only if it was not prevented, calls the shared
steps; `close()` calls them unconditionally (no `cancel` event, per spec).

New tests in `crates/js/src/dom/tests/v8_details_dialog_popover.rs`:
`dialog_request_close_removes_open`, `dialog_request_close_fires_cancel_then_close`,
`dialog_request_close_sets_return_value`, `dialog_request_close_preventable`,
`dialog_request_close_noop_when_not_open`.

`cargo test -p lumen-js --features v8-backend` green (dialog-related tests
17/17), `cargo clippy -p lumen-js --all-targets --features v8-backend -- -D warnings`
clean.
