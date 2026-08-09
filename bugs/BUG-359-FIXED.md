# BUG-359 — cross-document navigation never resolves relative URLs: `window.open("support/x.html")` and `location.href = "support/x.html"` both fail with `missing scheme`

**Статус:** FIXED 2026-08-09
**Компонент:** js (`crates/js/src/dom.rs` `window.open`, `_lumen_navigate_or_fragment`), shell (`crates/shell/src/main.rs:600-614` `resolve_js_navigation`, `crates/shell/src/main.rs:12836-12852` — the `window.open` popup drain, unchanged)
**Найден:** P2, WPT-VENDOR-encoding-detection (2026-07-27), `run_report.py --all --root encoding-detection --recursive`

## Симптом

A page served at `http://127.0.0.1:8300/encoding-detection/ar-ISO-8859-6-late.tentative.html`
calls

```js
var w = window.open("support/ar-ISO-8859-6-late.sub.html");
```

and the shell logs:

```
Reload: support/ar-ISO-8859-6-late.sub.html
Ошибка загрузки support/ar-ISO-8859-6-late.sub.html: invalid url: missing scheme: "support/ar-ISO-8859-6-late.sub.html"
```

The relative URL is handed to the network layer verbatim, never resolved against
the opener's document URL. The same holds for `location.href = "…"` /
`location.assign()` / `location.replace()` whenever the target is a *different*
document (see Причина).

This is the same failure family as BUG-347 (`fetch()` never resolves relative
URLs) and BUG-346 (`Url::resolve()` doesn't collapse `..`), but a third,
independent site: neither of those touches the navigation path.

## Причина

Two places drop the base URL, one of them after having already computed the
right answer:

1. **`window.open` (`dom.rs:11540-11544`)** stringifies its argument and passes
   it straight to `_lumen_window_open(url, target, features)` — no resolution
   step at all. The shell's drain (`main.rs:12836`) then calls
   `resolve_js_navigation(&url, &self.source)` (`main.rs:600`), whose entire
   non-`file://` branch is:

   ```rust
   if !url.starts_with("file://") {
       return Ok(PageSource::Url(url.to_owned()));
   }
   ```

   — the string is wrapped verbatim as a `PageSource::Url` and fails in the
   network layer.

2. **`_lumen_navigate_or_fragment` (`dom.rs:7450`)** — the shared backend of
   `location.href =`, `location.assign()` and `location.replace()` — *does*
   resolve correctly:

   ```js
   resolved = new URL(url, _lumen_loc_href).href;
   ```

   but only uses `resolved` to decide whether this is a same-document fragment
   navigation. On the cross-document path (`dom.rs:7475`) it discards it and
   calls `_lumen_navigate(url, replace)` with the **raw** argument. So the fix
   for this half is a one-token change (`url` → `resolved`, guarded on
   `resolved !== null`).

## Второй барьер на тех же тестах

Resolving the URL alone would not make the `-late` tests pass. They are
opener/popup round-trips: the popup does
`opener.postMessage(document.characterSet, "*")` and the opener asserts in
`window.onmessage`. Lumen's `window.open` returns a **stub object** whose own
comment says so (`dom.rs:11545-11546`, "Real cross-window messaging is not yet
supported"): `postMessage` is a no-op, `opener` is `null`, and the shell handles
the request by opening a *new tab* (`self.open_new_tab()`), which under the
single-window WPT executor navigates away from the test page entirely — hence
`Reload:` in the log. So even with correct URL resolution these tests would
still time out waiting for a message that can never arrive.

Filed as one bug because they share a single observable symptom and the same
call path; the messaging half is a known missing feature rather than a defect,
and is called out here only so a future fix isn't scoped to the URL half and
declared done.

## Масштаб

**31 of the 75 ids in `encoding-detection` — every `*-late.tentative.html`
test — TIMEOUT on this**, contributing ~5:10 of the category's 8:25 wall clock
(10 s harness timeout × 31). The other 44 ids executed and fail on BUG-358.

Beyond WPT: `window.open('relative/path')` is ordinary in real pages (help
popups, OAuth flows, print previews, "open in new tab" handlers), and the
`location.href = 'relative'` half affects every script that navigates with a
path rather than a full URL — a very common idiom in server-rendered apps.
BUG-347's own report already lists four earlier WPT categories where the same
`missing scheme:` signature was observed and shrugged off; this is the
navigation-side counterpart.

## Возможный фикс (не реализован в этой сессии)

- `dom.rs:7475`: pass `resolved` instead of `url` to `_lumen_navigate` when
  `resolved !== null`. Smallest possible change, fixes the whole `location`
  family.
- `dom.rs:11540`: resolve in `window.open` the same way
  (`new URL(url, _lumen_loc_href).href`, falling back to the raw string on
  throw) before calling `_lumen_window_open`, and use the resolved value for
  the stub's `location.href` too.
- Alternatively/additionally, do it once on the Rust side in
  `resolve_js_navigation` (`main.rs:600`) so every navigation entry point
  inherits it — that needs the opener's URL, which the function already receives
  as `&self.source`. Doing it in both places is harmless (resolution is
  idempotent for absolute URLs) and guards against a future third entry point.
- BUG-346 (`..` not collapsed in `Url::resolve()`) sits on the Rust-side
  resolution path; fix it first if the resolution is moved into Rust.
- Real popup support (a second browsing context with a live `opener` and working
  `postMessage`) is a much larger piece of work and is what the `-late` tests
  actually need — track separately if it is ever scheduled.

Not fixed in this session — P2-wpt vendors and surveys, code fixes are P3's lane
(`CLAUDE.md` developer assignments).

## Фикс (P3, 2026-08-09)

Both URL-resolution half-fixes from "Возможный фикс" applied, exactly as
scoped — the popup/`postMessage` half (second barrier) is intentionally left
alone, it's a separate, much larger feature (real second browsing context),
not a defect.

- `_lumen_navigate_or_fragment` (`dom.rs`, shared backend of `location.href =`
  / `.assign()` / `.replace()`): the cross-document branch now passes
  `resolved` (already computed via `new URL(url, _lumen_loc_href).href` a few
  lines above, previously used only to decide same-document-vs-cross-document
  and then discarded) to `_lumen_navigate` instead of the raw `url`, falling
  back to the raw string when `resolved` is `null` (invalid URL — unchanged
  behaviour, `_lumen_navigate` still receives something to report as an error
  downstream).
- `window.open` (`dom.rs`): resolves its `url` argument against
  `_lumen_loc_href` the same way, before calling `_lumen_window_open` — so
  both the queued popup request and the stub's `location.href` carry the
  resolved absolute URL. Falls back to the raw string on a `new URL()` throw
  (matches the `location` half's fallback).
- `resolve_js_navigation` (`main.rs:600`) and the popup drain (`main.rs:13183`)
  needed **no change** — both already assumed a pre-resolved absolute URL per
  their own doc comment ("relative URLs already resolved to absolute by the
  JS engine"); the bug was purely that the JS side never honoured that
  invariant.

5 new regression tests (`cargo test -p lumen-js --features v8-backend --lib`,
2519/2519 green): `location_assign_resolves_relative_url`,
`location_href_setter_resolves_relative_url`,
`location_replace_resolves_relative_url`,
`window_open_resolves_relative_url`,
`window_open_stub_location_href_resolves_relative_url` — each asserts the
queued `NavigateRequest`/`PopupRequest` carries the fully-resolved URL
(`https://example.com/dir/page.html` + `support/x.html` →
`https://example.com/dir/support/x.html`), reproducing the exact WPT idiom
from the symptom. `cargo clippy -p lumen-js --features v8-backend --all-targets
-- -D warnings` clean.

Residual: the `-late.tentative.html` popup/`postMessage` round-trip tests
still won't pass — that's the documented second barrier (no real second
browsing context, `window.open` returns a stub with a no-op `postMessage`),
tracked separately, not part of this bug's scope.
