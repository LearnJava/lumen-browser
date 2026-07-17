# BUG-296 — a live `lumen --bidi-port <N>` window's own default-homepage navigation can race and clobber an explicit early `browsingContext.navigate`

**Статус:** OPEN
**Компонент:** shell (startup/homepage navigation vs. BiDi-driven navigation, `crates/shell/src/main.rs`) — exact interaction not yet isolated
**Найден:** P2-wpt, re-verifying BUG-291's fix against `tests/wpt/run_smoke.py`, 2026-07-17

## Симптом

`tests/wpt/run_smoke.py` (and a hand-written direct BiDi driver, bypassing wptrunner) against
`/dom/nodes/Element-hasAttribute.html` times out waiting for `window.__lumen_wpt_results` — but
`document.readyState` reads back `"complete"` almost immediately, and stays `"complete"` for the entire
poll window (15–46s observed). `window.__lumen_wpt_results` never appears because the harness script
never appears to actually run against the intended page.

## Причина (partially isolated; not root-caused)

A plain `lumen --bidi-port <N>` process (no other CLI args — the exact invocation
`wptrunner.browsers.lumen.LumenBrowser`/`tests/wpt/run_smoke.py`/`verify_s3_bidi_session.py` all use)
begins its own default-homepage navigation on startup (observed loading `https://ria.ru/` in this
environment) independent of any BiDi command. The test driver's `browsingContext.navigate` for the
intended test URL is issued shortly after connecting. Log interleaving shows the homepage's response
(`← 200 https://ria.ru/`, streaming/pipeline-finish for the homepage) landing chronologically **after**
the test page's own `← 200 .../Element-hasAttribute.html` and script loads — i.e. the homepage load
appears to finish loading into the same top-level browsing context *after* the intended navigation
completed, leaving `window`/`document` pointing at the homepage rather than the test page for any
subsequent `script.evaluate`.

Ruled out as the cause: stale `last_session.db`/`settings.db` restoring a previously-bookmarked/visited
`ria.ru` — reproduces identically with both files freshly deleted (a brand-new, empty `data/` directory)
immediately before the run. No hardcoded `ria.ru` literal exists in `crates/shell/src/*.rs`
(`grep -rn ria.ru` across the crate is empty), so the source of the default navigation itself is not yet
identified either — could be a network-based default-homepage/geo redirect, a `newtab.rs` fallback, or
config sourced from somewhere outside this worktree's tracked files.

This matches the class of issue already noted in `CLAUDE.md` → "Known gotchas" ("Live-window BiDi/MCP
`script.evaluate` can hang indefinitely in some working sessions" — every `script.evaluate` reports
`"JS context not available"` forever past the normal bounded-retry install race). That note describes the
*symptom* (eval never resolving); this bug narrows the *mechanism* one step further (a second, later
navigation into the same context) without yet reaching a fix.

## Repro

1. Build `lumen.exe` (`dev-release`) with the BUG-291 fix applied (or any recent build — this reproduces
   independent of BUG-291's DOM-wrapper-identity fix).
2. `LUMEN_PROFILE=dev-release <venv>/python tests/wpt/run_smoke.py` — times out waiting for
   `testharnessreport.js` results, even though the test page's own resources (`testharness.js`,
   `testharnessreport.js`, the test HTML itself) are all successfully fetched (visible in stdout).
3. Or drive directly over BiDi: spawn `lumen --bidi-port <N>` fresh (empty `data/` dir), connect a
   `BidiSession`, `browsingContext.navigate` to any local test URL, then poll
   `script.evaluate("document.readyState")` — reads `"complete"` immediately but polling any page-specific
   global (e.g. `window.__lumen_wpt_results`) never returns it.

## Что нужно для закрытия

1. Identify where the default-homepage navigation on a bare `lumen --bidi-port <N>` invocation (no URL
   argument) actually originates — despite the CLI help text describing this invocation as "пустое окно"
   (empty window). Check `crates/shell/src/newtab.rs`/`main.rs` startup-navigation logic and whether it's
   gated on anything that a WPT/automation harness should be disabling (e.g. an explicit `--blank`/no-homepage
   flag, or automation should imply `about:blank` startup instead of a real homepage fetch).
2. Once the source is found, either (a) make `lumen --bidi-port <N>` with no URL argument start truly
   blank (matching its documented behavior) so no navigation races an automation driver's first
   `browsingContext.navigate`, or (b) if a homepage load is intentional even in this mode, make
   `browsingContext.navigate` authoritative — a BiDi-issued navigation after connect should always win the
   race against a same-process startup navigation, not silently lose to it.
3. Re-run `tests/wpt/run_smoke.py` — DoD unblocks the S4 checkbox at
   `docs/tasks/p2-wpt-integration.md` ("A deliberately-failing assertion is observed as FAIL").
