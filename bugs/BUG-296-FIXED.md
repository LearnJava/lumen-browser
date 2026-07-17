# BUG-296 — a live `lumen --bidi-port <N>` window's own default-homepage navigation can race and clobber an explicit early `browsingContext.navigate`

**Статус:** FIXED 2026-07-17 — root cause identified and fixed (stale on-disk session restore, not a "default homepage" feature). `tests/wpt/run_smoke.py` still doesn't reach a real PASS in this environment — a separate, pre-existing, environment-dependent blocker (see "Остаток" below), already tracked in `CLAUDE.md` → "Known gotchas".
**Компонент:** shell (session restore vs. automation launch, `crates/shell/src/main.rs`, `crates/shell/src/session_persist.rs`)
**Найден:** P2-wpt, re-verifying BUG-291's fix against `tests/wpt/run_smoke.py`, 2026-07-17

## Симптом

`tests/wpt/run_smoke.py` (and a hand-written direct BiDi driver, bypassing wptrunner) against
`/dom/nodes/Element-hasAttribute.html` times out waiting for `window.__lumen_wpt_results` — but
`document.readyState` reads back `"complete"` almost immediately, and stays `"complete"` for the entire
poll window (15–46s observed). `window.__lumen_wpt_results` never appears because the harness script
never appears to actually run against the intended page.

## Причина (confirmed)

`lumen`'s **cross-restart session restore** (§10I, `Lumen::restore_session` in `crates/shell/src/main.rs`),
not a "default homepage" feature — there is no homepage/search-engine config in this codebase at all
(`grep -rn "homepage\|start_url" crates/shell/src/config.rs` is empty). Any argument-less launch
(`PageSource::Empty`, which is exactly what `lumen --bidi-port <N>` with no URL is) unconditionally called
`app.restore_session()`, which reopens whatever tab was active in the *last saved session* — read from
`session_persist::SESSION_DB_PATH = "last_session.db"`, **a bare filename resolved against the process's
current working directory**, not the portable `<exe_dir>/data/` convention every other subsystem uses
(`crates/shell/src/adblock.rs::browser_data_dir`). Confirmed by direct inspection: the repo root had a
real leftover `last_session.db` (from an earlier interactive `cargo run -p lumen-shell -- ...` /
manually-launched `lumen` session, saved on window close) whose single active tab was `https://ria.ru`:

```
$ python -c "import sqlite3; print(list(sqlite3.connect('last_session.db').execute(
    'SELECT url, is_active FROM session_tabs')))"
[('https://ria.ru', 1)]
```

`wptrunner.browsers.lumen.LumenBrowser.make_command` spawns `[binary, "--bidi-port", str(port)]` with no
`cwd` override, so it inherits the test runner's CWD — the repo root, the same directory a developer
would have run `lumen` from interactively. On the next `--bidi-port` launch, `restore_session()` silently
reopened that same `ria.ru` tab and began fetching it in the background, landing in the same top-level
context the automation driver's own `browsingContext.navigate` had just been issued against — sometimes
*after* the intended navigation completed, leaving `window`/`document` pointing at `ria.ru` for any
subsequent `script.evaluate`.

Why the original diagnosis's "ruled out: fresh `data/` dir reproduces identically" didn't catch this:
`last_session.db` lives in the CWD (repo root), **not** inside `<exe_dir>/data/` — deleting `data/` never
touched it, so the mitigation attempt looked like it ruled out session state when it had never actually
been tested against a truly session-free launch.

This is also a *pre-existing* inconsistency independent of automation: `lumen`'s own CLI help
(`print_usage`) and doc comment both describe an argument-less launch as "пустое окно" (empty window),
but session restore silently made that untrue whenever a saved session existed — automation just made the
inconsistency observable as a hard race.

## Repro

1. Build `lumen.exe` (`dev-release`).
2. Ensure a `last_session.db` with a saved active tab exists in the launch CWD (e.g. close a normal
   interactive `lumen`/`cargo run -p lumen-shell` window after visiting any page from the repo root).
3. Spawn `lumen --bidi-port <N>` from that same CWD, connect a `BidiSession`, issue
   `browsingContext.navigate` to a distinct local test URL immediately, then poll
   `script.evaluate("document.location.href")` — observed the stale session's URL, not the navigated one,
   in some runs (timing-dependent race, not deterministic every time — matches the original report's
   "sometimes lands after").

## Фикс (2026-07-17)

`crates/shell/src/main.rs`: new pure helper `should_restore_session(source: &PageSource, automation_mode:
bool) -> bool` — `true` only for `PageSource::Empty` (no explicit page arg) **and** no automation
front-end attached. `run_window_mode` gained an `automation_mode: bool` parameter
(`bidi_port.is_some() || mcp_live_port.is_some()`, computed in `main()` where both are already in scope)
and now calls `app.restore_session()` only when `should_restore_session` returns `true`. This matches
option (a) from this bug's original remediation plan: `--bidi-port`/`--mcp-live-port` launches now
actually start blank, as already documented, instead of silently inheriting whatever tab a prior
interactive run left behind.

Verified two ways:
1. Unit tests (`should_restore_session_empty_source_no_automation`,
   `should_restore_session_skipped_in_automation_mode`,
   `should_restore_session_skipped_for_explicit_source`, `crates/shell/src/main.rs`).
2. Direct BiDi repro (scratch script, not committed): spawned `lumen --bidi-port <N>` with CWD seeded with
   a `last_session.db` pointing at `https://example-stale-tab.test/`, issued
   `browsingContext.navigate` to a local test page immediately after connect. Network log shows exactly
   one `GET` — the navigated test page — with **no** request to `example-stale-tab.test` at any point
   (previously this would race in the fetch log). `cargo test -p lumen-shell --bin lumen
   should_restore_session` and `cargo clippy -p lumen-shell --all-targets -- -D warnings` clean.

## Остаток

Re-running `tests/wpt/run_smoke.py` after this fix (both with and without a stale `last_session.db`
present) still times out in this environment — but on a **different, already-documented** symptom:
`script.evaluate` never resolves (`"JS context not available"` past the bounded-retry window), matching
`CLAUDE.md` → "Known gotchas" → "Live-window BiDi/MCP `script.evaluate` can hang indefinitely in some
working sessions", which is explicitly independent of any specific navigation content and was already
flagged as out of scope to root-cause inside an unrelated task. The direct-BiDi repro above confirms this
bug's own mechanism (stale-session race) is gone — the network log shows only the intended navigation's
fetch, never the stale tab's — so the S4 DoD checkbox in `docs/tasks/p2-wpt-integration.md` is unblocked
from *this* bug specifically; the JS-context-install gap is a separate, pre-existing blocker for reaching
an actual `run_smoke.py` PASS and needs its own investigation.
