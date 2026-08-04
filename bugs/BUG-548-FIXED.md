# BUG-548: cookie-banner auto-dismiss (privacy feature) is a no-op under the default V8 build — the user-facing toggle silently does nothing

**Статус:** FIXED 2026-08-04
**Дата:** 2026-08-03
**Компонент:** js (`crates/js/src/cookie_banner.rs`, install site `crates/js/src/lib.rs:1030` — QuickJS-only) + shell (`crates/shell/src/main.rs`)
**Найден:** P1, S12b-G0 триаж (13 модулей без V8-порта)

## Механизм

`cookie_banner::install` (a `MutationObserver`-based shim that watches for
EasyList "I don't care about cookies" consent-banner selectors and
auto-clicks Accept) is only called from `QuickJsRuntime::install_dom`. The
shell-side flag that drives it, `cookie_banner_dismiss` (defaults to `true`,
user-toggleable via `KeyCommand::ToggleCookieBannerDismiss`,
`crates/shell/src/main.rs:18538`), is only ever pushed into the runtime via
`rt.set_cookie_banner_dismiss(...)` — a method that exists on
`QuickJsRuntime` but has no `V8JsRuntime` equivalent. This is already
self-documented in the shell code at `main.rs:19230-19236` ("Cookie-banner
dismiss is not wired for V8 yet") but was never turned into a tracked bug
or mentioned in `CAPABILITIES.md`.

## Симптом

`Ctrl` binding `KeyCommand::ToggleCookieBannerDismiss` and the
`cookie_banner_dismiss: true` default both suggest this is an active,
shipped privacy feature — on the default (V8) build it has zero effect:
consent banners render and stay visible exactly as authored, regardless of
the toggle state. Not mentioned in `CAPABILITIES.md`, so no doc overclaim
to fix, but the feature itself is silently broken for every user on the
default build.

## Фикс

**Закрыт 2026-08-04 (P1, S12b-G6).** Ported per the standard S12b-G group
procedure (`docs/tasks/p1-s12b-cleanup-queue.md` §4): no natives — the
familiar fast path, `install_cookie_banner_bindings_v8` calling
`rt.eval(COOKIE_BANNER_SHIM)`, registered as an extra-arg call site in
`v8_runtime.rs::install_dom` (the enable flag lives on `self`, so the plain
`install_v8!` macro doesn't fit). Added `cookie_banner_dismiss: AtomicBool`
+ `set_cookie_banner_dismiss()` on `V8JsRuntime`, mirroring
`QuickJsRuntime`'s field/method of the same name — both shell call sites in
`crates/shell/src/main.rs` (`run_scripts_with_dom`'s classic-load branch and
the `Lumen::` navigate path) now call `rt.set_cookie_banner_dismiss(...)` on
the V8 branch too, so `KeyCommand::ToggleCookieBannerDismiss` has an effect
again. 12 of 16 existing tests ported against `V8JsRuntime` (4 pure-Rust
selector-list tests needed no engine, left ungated). rquickjs side removed
in the same batch. Details — `docs/tasks/ph3-v8-migration.md` §S12b-G6.
