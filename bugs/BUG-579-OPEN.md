# BUG-579: `HTMLDialogElement.prototype.requestClose()` missing entirely

**Статус:** OPEN
**Компонент:** js (`crates/js/src/dom.rs:5667-5702` — the `<dialog>` API
block has `show`/`showModal`/`close`/`returnValue` but no `requestClose`)
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
