# BUG-545: `document.startViewTransition()` does not exist under the default V8 build — SPA View Transitions is dead despite being marked "done"

**Статус:** OPEN
**Дата:** 2026-08-03
**Компонент:** js (`crates/js/src/view_transitions.rs`, install site `crates/js/src/lib.rs:1123` — QuickJS-only)
**Найден:** P1, S12b-G0 триаж (13 модулей без V8-порта)

## Механизм

`view_transitions::install_view_transition_bindings` (the JS shim defining
`document.startViewTransition(callback)`, `ViewTransition` and its
`_lumen_vt_begin`/`_lumen_vt_end`/`_lumen_vt_cancel` native hooks the shell
drains to drive the cross-fade) is only called from
`QuickJsRuntime::install_dom` — there is no `install_view_transition_bindings_v8`
and no call from `V8JsRuntime`'s install path. `grep -n "view_transitions::"
crates/js/src/v8_runtime.rs` — zero hits.

The engine-side mechanism this JS API drives (`P3-viewtransnav`/`P2-viewtrans`
in `ROADMAP.md:109`, root cross-fade + `::view-transition-*` pseudo-elements)
is itself fully wired and V8-agnostic — it's the JS *trigger* that's missing,
not the rendering.

## Симптом

`ROADMAP.md:109` marks `P2-viewtrans` (View Transitions API, same-document
SPA) as **done**, and `CAPABILITIES.md`'s Misc bullet lists "View
Transitions" under a run-on ✅ list — both claims are false on the shipped
default (V8) build: `typeof document.startViewTransition` → `"undefined"`.
Any page using the standard SPA view-transition idiom
(`if (document.startViewTransition) { document.startViewTransition(() =>
updateDOM()); } else { updateDOM(); }`) silently falls back to the
no-transition branch — the feature-detection itself is correct, so this
fails invisibly rather than throwing, which is why it went unnoticed since
the S12 cutover (2026-07-14).

## Фикс (не сделан)

Port per the standard S12b-G group procedure (`docs/tasks/p1-s12b-cleanup-queue.md`
§4): add `install_view_transition_bindings_v8` next to the existing
QuickJS-side function, register via `install_v8!` in
`v8_runtime.rs::install_dom`, port the 11 existing tests against
`V8JsRuntime`. The native `_lumen_vt_begin`/`_lumen_vt_end`/`_lumen_vt_cancel`
binding pattern already has V8 precedent elsewhere in the crate — no new
mechanism needed, just wiring.
