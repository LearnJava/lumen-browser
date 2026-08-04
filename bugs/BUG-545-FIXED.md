# BUG-545: `document.startViewTransition()` does not exist under the default V8 build — SPA View Transitions is dead despite being marked "done"

**Статус:** FIXED 2026-08-04
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

## Фикс

**Закрыт 2026-08-04 (P1, S12b-G5).** Ported per the standard S12b-G group
procedure (`docs/tasks/p1-s12b-cleanup-queue.md` §4): added
`install_view_transition_bindings_v8`, registered as an extra-arg call in
`v8_runtime.rs::install_dom` (mirrors the `pointer_capture`/`geolocation`
pattern — the 3 natives need the runtime-instance
`view_transition_events: Arc<Mutex<Vec<ViewTransitionEvent>>>`, a new
`V8JsRuntime` field mirroring `fullscreen_requests`, not just `&self`).
`_lumen_vt_begin`/`_lumen_vt_end`/`_lumen_vt_cancel` registered via
`register_native`/`into_v8_fn0`. All 11 existing tests ported against
`V8JsRuntime`. The shell's `V8PersistentJs::take_view_transition_events`
(`crates/shell/src/main.rs`), previously a hardcoded `Vec::new()` stub, now
calls through to the new `V8JsRuntime::take_view_transition_events()` —
this was the second half of the regression (the JS API existed nowhere
under V8, but even a ported JS side would have been silently dropped by
this stub). rquickjs side removed in the same batch (`QuickJsRuntime` loses
`document.startViewTransition` — accepted per the established G-group
side effect, rquickjs is a frozen rollback path being deleted piecewise).
`CAPABILITIES.md` updated: View Transitions moved from the 🟡 QuickJS-only
caveat into the main Misc ✅ list.
