# BUG-582: Invoker Commands API (`command`/`commandfor` on `<button>`, `CommandEvent`) not implemented at all

**Статус:** OPEN
**Компонент:** js (`crates/js/src/dom.rs` — no `command`/`commandFor`
reflection, no `CommandEvent` constructor, no invoke-action dispatch logic
anywhere in the file; the only `command`-adjacent code is
`document.execCommand` → `_lumen_exec_command` at `dom.rs:2921`/`7314`, the
unrelated legacy editing API)
**Найден:** P2, WPT-VENDOR-html-semantics-misc, 2026-08-04

## Симптом

```
FAIL <test name> - CommandEvent is not defined
```

84 occurrences, entirely within `the-button-element/command-and-commandfor/`.

## Причина

The Invoker Commands API (HTML LS §4.10.9, shipped behind
`command-and-commandfor` in the vendored test path — a 2024/2025 spec
addition letting `<button command="show-popover" commandfor="idX">`
declaratively control popovers/dialogs without a script handler) has no
implementation surface: no `command`/`commandFor` IDL reflection on
`HTMLButtonElement`, no `invoke()` action-dispatch algorithm that would
fire a `command` event on the target element when such a button is
activated, and no `CommandEvent` interface (`event.command`/`event.source`)
at all. `<button popovertarget=… popovertargetaction=…>` (the older, narrower
popover-only mechanism) does work — see `dom.rs:15096` — but the newer
generalized `command`/`commandfor` pair that also covers `<dialog>`
show/close/request-close and custom `command="--foo"` values is a distinct,
unimplemented feature.

## Масштаб

Whole feature, self-contained to `the-button-element/command-and-commandfor/`.
Depends on [BUG-579](bugs/BUG-579-OPEN.md) (`dialog.requestClose()`) for the
dialog-target subset of its own test matrix, since several
`command-and-commandfor` subtests target `<dialog>` with built-in
`command="request-close"` values.
