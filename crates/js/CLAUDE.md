# crates/js — context for JS / Web-API work

Loaded when a session works under `crates/js/`. Root rules are in [`/CLAUDE.md`](../../CLAUDE.md).

- **V8 (`rusty_v8`) is the only JS engine.** `rquickjs`/QuickJS is gone from the workspace — never target it with a fix or an investigation, even where an old doc or comment still names it.
- **The shared shim is `src/shim/*.js`** (one file per `WEB_API_SHIM*` const). Edit the `.js`, not `dom.rs`. The files are read verbatim — nothing in them is escaped.
- **Per-feature modules (`xhr.rs`, `worker.rs`, `web_audio.rs`, …) install their own JS**, which a page-shim fix never reaches. A fix that assumes one edit covers all of them is incomplete.
- **Read [`subsystems/js.md`](../../subsystems/js.md) §Invariants before changing event dispatch, the runtime thread or the shims** — it is the trap list for this crate.
- The `navigator.userAgent` literal in `src/shim/web_api_shim_mid_b.js` is the one version string maintained by hand; everything else derives from `CARGO_PKG_VERSION`.
