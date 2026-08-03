# BUG-548: cookie-banner auto-dismiss (privacy feature) is a no-op under the default V8 build — the user-facing toggle silently does nothing

**Статус:** OPEN
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

## Фикс (не сделан)

Port per the standard S12b-G group procedure
(`docs/tasks/p1-s12b-cleanup-queue.md` §4, S12b-G6 slot): add an
`install_cookie_banner_v8` (or fold the shim into the shared engine-agnostic
`WEB_API_SHIM` in `dom.rs`, since it has no native bindings beyond the
enable flag), and give `V8JsRuntime` a `set_cookie_banner_dismiss` matching
`QuickJsRuntime`'s so the shell's existing call sites (`main.rs:7283`,
`19193`) can be mirrored for the V8 branch without special-casing.
