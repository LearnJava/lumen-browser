# Web Platform Tests (WPT) — Lumen integration

P2-wpt (`docs/tasks/p2-wpt-integration.md`, slices S1–S8). Runs the real, unmodified
`wptrunner` against Lumen over WebDriver BiDi (`lumen --bidi-port N`) — not a
bespoke test runner. See the task doc for the full architecture and slice plan.

**Status:** S1–S7 done (this task complete; S8 reftests is a separate
follow-up). `tests/wpt/run_smoke.py` drives the real, unmodified `wptrunner`
against `lumen --bidi-port` (navigate + eval) end to end; `tests/wpt/run_suite.py`
(S7) runs the **whole curated subset** — the 18 synchronous `dom/nodes/` tests
(S5) plus the 3 async `MutationObserver-*` tests (S6), 21 tests / 64 checks —
as one pass/fail gate against its committed `.ini` expectations, **green**
(0 unexpected), fully offline. See "The curated subset" and "Running the whole
suite" below. Nine real engine/shell gaps surfaced and were fixed while
proving the S4 path (the last blocker, [BUG-301](../../bugs/BUG-301-FIXED.md),
was fixed 2026-07-18): [BUG-278](../../bugs/BUG-278-FIXED.md)
(HTTP client rejected `wptserve`'s close-delimited responses), [BUG-279](../../bugs/BUG-279-FIXED.md)
(`document.getElementsByTagName` was missing entirely — broke `testharness.js`'s
own module-level setup), [BUG-280](../../bugs/BUG-280-FIXED.md) (`window` wasn't
the JS engine's real global object, so `testharness.js`'s `expose()`-based public API
was unreachable as bare identifiers), [BUG-291](../../bugs/BUG-291-FIXED.md) (DOM
node wrappers weren't interned, breaking `===` node identity and crashing
`testharness.js`'s built-in results renderer, `Output.show_results`),
[BUG-296](../../bugs/BUG-296-FIXED.md) (a stale on-disk `last_session.db` — session
restore, not a "default homepage" feature — could reopen a leftover tab and race the
test driver's explicit `browsingContext.navigate`; `--bidi-port`/`--mcp-live-port`
launches now skip session restore), [BUG-298](../../bugs/BUG-298-FIXED.md)
(`Element`/`DocumentFragment`/`ShadowRoot`.querySelector(All) searched the whole
document instead of the calling node's subtree — `Output.show_results` builds a
detached results tree and queries into it, always getting nothing),
[BUG-299](../../bugs/BUG-299-FIXED.md) (`Element.prototype.insertAdjacentText` was
missing entirely, thrown from the same code path), [BUG-300](../../bugs/BUG-300-FIXED.md)
(`browsingContext.navigate`'s `DocumentReady` wait could ACK using the *previous*
page's stale `layout_box` before the new page had even started loading), and
[BUG-301](../../bugs/BUG-301-FIXED.md) (`wptrunner`'s vendored harness registers its
own static route for `/resources/testharnessreport.js`, winning over Lumen's
vendored file that sets `window.__lumen_wpt_results` — plus a related, independently
found persistent-on-disk-HTTP-cache angle on the same symptom, see
[BUG-315](../../bugs/BUG-315-FIXED.md)). Together BUG-298/299/300 fully explain (and
disprove as environment-flaky) the "`script.evaluate`-install race" theory
previously in `CLAUDE.md` → "Known gotchas". See those bug files and the task doc's
S4 section for the full diagnosis trail (BiDi-eval-based bisection of
`testharness.js`'s execution).

## What's here

- `tools/wptrunner/`, `tools/manifest/`, `tools/serve/`, `tools/wptserve/`,
  `tools/webdriver/`, `tools/metadata/`, `tools/gitignore/`, `tools/localpaths.py`
  (repo root, alongside `crates/`) — vendored upstream WPT tooling. Pin +
  rationale: `tests/wpt/VENDOR.md`. **Not upstream-unmodified in one spot:**
  `tools/wptrunner/wptrunner/products.py`'s `BUILTIN_PRODUCTS` frozenset has a
  `"lumen"` entry added (see `docs/tasks/p2-wpt-integration.md` S3 DoD) — there is
  no plugin-registration seam that avoids touching this file, so a re-vendor must
  reapply that one line.
- `tools/wptrunner/wptrunner/browsers/lumen.py` — **ours** — the wptrunner
  product plugin: `LumenBrowser` (spawn/stop `lumen --bidi-port <port>`,
  `WebDriverBrowser` subclass) + `__wptrunner__` registration.
- `tools/wptrunner/wptrunner/executors/executorlumen.py` — **ours** —
  `LumenBidiProtocol` (BiDi-only session negotiation via
  `webdriver.bidi.client.BidiSession.bidi_only`, no classic HTTP session) and
  `LumenTestharnessExecutor.do_test` (S4): `browsingContext.navigate` then
  `script.evaluate`-polls for `tests/wpt/resources/testharnessreport.js`'s
  JSON result global, tolerating the transient "JS context not available"
  BiDi error while the new document's JS runtime is still installing.
- `tests/wpt/resources/testharness.js` — vendored upstream client-side test harness.
- `tests/wpt/resources/testharnessreport.js` — **ours** (S4) — on harness
  completion, serializes `[url, harness_status, message, stack, subtests]` to
  JSON on `window.__lumen_wpt_results`, polled by `do_test` above.
- `tests/wpt/dom/nodes/` — one vendored test category. S4's smoke test is
  `Element-hasAttribute.html` (not `Document-createElement.html`, floated as an
  "e.g." example when this file was first drafted — turned out to need
  un-vendored iframe fixtures and `async_test`, not actually trivial).
- `tests/wpt/requirements.txt` — pip requirements to make the above importable.
- `tests/wpt/verify_s3_bidi_session.py` — S3 verification: spawns a real
  `lumen --bidi-port <port>` and confirms BiDi session negotiation succeeds
  (real `sessionId` + `capabilities`). Run with:

  ```bash
  LUMEN_PROFILE=dev-release <venv>/python tests/wpt/verify_s3_bidi_session.py
  ```

- `tests/wpt/verify_devx6_bidi_scenarios.py` — **ours** (DEVX-6, `ROADMAP.md`) —
  integration scenario tests for six previously-unused BiDi commands
  (`network.setOfflineStatus`, `network.addIntercept`+`failRequest`/
  `continueRequest`, `browser.setTimezoneOverride`,
  `emulation.setUserAgentOverride`) against a real spawned `lumen --bidi-port`
  window, same raw `BidiSession` pattern as `verify_s3_bidi_session.py`
  (not wptrunner). Checks two things per command: the protocol round-trip
  (real verification value, catches `lumen-bidi-server` regressions) and
  whether a live page actually observes the effect — confirmed **not wired**
  today ([BUG-295](../../bugs/BUG-295-OPEN.md), reported as `XFAIL(BUG-295)`,
  not a script failure). Also documents a separate, environment-dependent gap
  found while writing it: the live window's JS runtime can fail to install at
  all in some sessions (`SKIP(env)` — see `CLAUDE.md` "Known gotchas"). Run
  with:

  ```bash
  LUMEN_PROFILE=dev-release <venv>/python tests/wpt/verify_devx6_bidi_scenarios.py
  ```

- `tests/wpt/run_smoke.py` — **ours** (S4) — minimal driver that calls
  `wptcommandline`/`wptrunner.run_tests` directly against the smoke test (see
  its own docstring for why this isn't `tools/wpt/wpt`). Passes
  `--no-restart-on-unexpected`: wptrunner's own default respawns the browser
  process after every test whose result doesn't match its expectation, which
  under `--all` (no committed `.ini` for most tests) meant a fresh `lumen.exe`
  per failing test. One `lumen.exe` process now runs the whole selected test
  set, reusing a single browsing context (`LumenBidiProtocol.context_id`,
  `executorlumen.py`) that gets a fresh `browsingContext.navigate` per test —
  still test-isolated, just not process-isolated. The browser still restarts
  on an actual crash/hang. Run with:

  ```bash
  LUMEN_PROFILE=dev-release <venv>/python tests/wpt/run_smoke.py
  ```

  Both scripts default to `target/<LUMEN_PROFILE>/lumen.exe` (`LUMEN_PROFILE`
  env var, default `release`), same convention as `graphic_tests/run.py`.
- `tests/wpt/run_suite.py` — **ours** (S7) — the CI wrapper: discovers the
  curated subset from the committed `metadata/dom/nodes/*.ini` expectations
  (one test id per `.ini`) and runs them all through `run_smoke.run()` as a
  single pass/fail gate — exit 0 iff 0 unexpected results. This is the
  repeatable local/CI invocation (see "Running the whole suite" below). Adding
  an `.ini` grows the gate; there is no separate list to maintain.
- `tests/wpt/run_report.py` — **ours** — HTML report, not a gate: always runs
  and always exits 0 (unless the run itself couldn't start), writing a
  self-contained `.tmp/wpt-report.html` (test/subtest counts, pass/fail per
  test, expandable per-subtest detail with the failure message, and whether
  each result matched its `.ini` expectation). Defaults to the same curated
  subset as `run_suite.py`; `--all` instead runs every vendored `.html` under
  `dom/nodes/` (168 files, not just the 20 curated ones) — most of those were
  never vetted for this minimal BiDi-only executor (no `test_driver.*`,
  multi-window, iframes), so expect ERROR/TIMEOUT/FAIL noise there, not bugs
  worth filing without individually checking first; use it to survey, not to
  gate. See "HTML report" below.
- `tests/wpt/config.json` — **ours** (S4) — `wptserve` config override: pins
  `browser_host` to `127.0.0.1` (the default, `web-platform.test`, needs
  `/etc/hosts` entries this task's "no live network" rule can't rely on) and
  disables the `wss`/`h2`/`webtransport-h3`/`dns` servers the smoke test
  doesn't need (Python 3.14's `ssl` module dropped `wrap_socket`, breaking
  `wptserve`'s `wss` server; unrelated to Lumen). HTTP ports are `8300`/`8301`,
  not the WPT default `8000`/`8001` — the 8000-range falls inside a Windows
  dynamic excluded-port range here (`netsh interface ipv4 show
  excludedportrange protocol=tcp`), so `wptserve` failed to bind with
  `WinError 10013`.
- `tests/wpt/metadata/` — `--metadata` root; holds the generated (gitignored)
  `MANIFEST.json` and the committed `.ini` expectations (S5 onward, under
  `metadata/dom/nodes/`).

## Python setup

Requires Python 3.9+ (verified against 3.14). From the repo root:

```bash
python -m venv .venv-wpt          # any venv location outside the repo's gitignored area works
.venv-wpt/Scripts/python -m pip install -r tests/wpt/requirements.txt   # Windows
# .venv-wpt/bin/python -m pip install -r tests/wpt/requirements.txt    # Linux/macOS
```

This is tooling setup only — not a Cargo dependency, no `docs/plan/tech-stack.md`
entry needed (see that file's dependency-policy scope: Rust deps only).

### Verifying the install (import-chain smoke check)

Confirms the vendored tree + pip deps actually resolve, cheaper than a full
`run_smoke.py` run when only checking the Python side:

```bash
python - <<'PY'
import sys, os
root = os.path.abspath(".")
here = os.path.join(root, "tools")
sys.path[:0] = [root, here, os.path.join(here, "wptserve"),
                os.path.join(here, "webdriver"), os.path.join(here, "wptrunner")]

import localpaths                       # noqa: F401  (repo_root bootstrap)
import manifest.manifest                # noqa: F401  (test manifest)
from tools.serve import serve           # noqa: F401  (local HTTP/WS test server)
import wptrunner.wptrunner              # noqa: F401  (the runner)
import wptrunner.wptcommandline         # noqa: F401  (CLI arg parsing)
from webdriver.bidi.client import BidiSession  # noqa: F401  (S3 will drive this)
print("wptrunner import chain OK")
PY
```

Expected output: `wptrunner import chain OK`. This is exactly the import closure
`tools/wptrunner`'s own module-load time touches (`environment.py` →
`tools.serve.serve`, `testloader.py`/`metadata.py` → `manifest.manifest`) — if any
of it breaks after a re-vendor or a dependency bump, this is where it'll show up
first, before anything BiDi-specific.

## Fully offline

Once `pip install -r tests/wpt/requirements.txt` has populated the venv, nothing
above touches the network — the vendored tree in `tools/`/`tests/wpt/` is a
committed snapshot (`tests/wpt/VENDOR.md`), not a submodule or a runtime clone.

## The curated subset (S5 + S6)

`metadata/dom/nodes/*.html.ini` pins the expected result of a curated set of
**21 `dom/nodes/` tests** — 18 fully-synchronous ones (S5; no iframes / XHR /
`testdriver` / `async_test`) plus 3 async `MutationObserver-*` tests (S6;
`promise_test`/`async_test`, the only self-contained async tests in the
vendored `dom/` corpus). Every genuine failure is recorded as `expected: FAIL`
(and one whole-harness `expected: TIMEOUT`, `MutationObserver-disconnect.html`),
so the whole set runs **green** (0 unexpected) and acts as a regression ratchet
— same idea as `KNOWN_DEBTORS` in `graphic_tests/`, but tool-native. Each `.ini`
is header-commented with the engine bug it tracks (BUG-302/309/310/311/312/
313/314 for S5; BUG-317/318/319 for S6); flip a `FAIL` to `PASS` in the same
commit that lands the fix.

## Running the whole suite (S7 gate)

`tests/wpt/run_suite.py` is the repeatable local/CI invocation: it discovers
the curated subset from the committed `.ini` files (no hand-kept test list) and
runs it as one pass/fail gate. On Windows Git Bash set
`MSYS2_ARG_CONV_EXCL='/dom'` so the leading-slash test IDs the runner emits
aren't mangled into Windows paths:

```bash
export LUMEN_PROFILE=dev-release MSYS2_ARG_CONV_EXCL='/dom'
BIN=$(cygpath -w "$PWD/target/dev-release/lumen.exe")
tests/wpt/.venv/Scripts/python.exe tests/wpt/run_suite.py --binary "$BIN"
# → "running 20 curated WPT tests" then
#   "Ran 61 checks (41 subtests, 20 tests) ... Unexpected results: 0", exit 0
```

The curated-subset size drifts as tests are added/excluded (see "Adding a test"
below) — don't hardcode it elsewhere; `run_suite.py --binary "$BIN"` always
prints the current count.

## HTML report

`tests/wpt/run_report.py` runs the same curated subset (or, with `--all`,
every vendored `dom/nodes/*.html` test) and writes a self-contained HTML file
— open it in any browser, no server needed. Unlike `run_suite.py` it's not a
gate: it always writes the report and exits 0 regardless of how many tests
failed, so use it to *look at* results rather than to fail a build.

```bash
export LUMEN_PROFILE=dev-release MSYS2_ARG_CONV_EXCL='/dom'
BIN=$(cygpath -w "$PWD/target/dev-release/lumen.exe")
tests/wpt/.venv/Scripts/python.exe tests/wpt/run_report.py --binary "$BIN" --out .tmp/wpt-report.html
# → "tests: 20/20 harness OK; subtests: 35/41 passed"
#   "report written to .tmp/wpt-report.html"
```

The report shows, per test: harness status (`OK`/`ERROR`/`TIMEOUT`/`CRASH`),
subtests passed/total, and duration; expand a row for the per-subtest
breakdown with failure messages. Summary cards at the top separate "raw"
pass/fail counts from "unexpected (vs `.ini`)" — a subtest can legitimately
`FAIL` while still being 0 unexpected, if that failure is pinned as
`expected: FAIL` (a tracked, known gap) rather than a surprise regression.

`--all` runs every vendored/generatable test under `--root` (default
`dom/nodes`, 168 files) instead of just the 20 curated ones — most were never
vetted for this project's minimal BiDi-only executor (no `test_driver.*`,
multi-window, iframes), so expect a lot of ERROR/TIMEOUT/FAIL noise there.
Useful to survey what else might be worth curating next; don't file bugs off
it without checking each failure individually first (same discipline as
"Adding a test" below) — and budget more time for it (no per-test
parallelism). Pass `--root FileAPI` (or any other vendored category under
`tests/wpt/`) plus `--recursive` to survey a category organized into
subdirectories — `--recursive` walks the directory tree and expands
`.any.js`/`.window.js` templates into their `.any.html`/`.window.html` ids
the way `wptserve`'s `AnyHtmlHandler`/`WindowHandler` do at request time,
skipping `support`/`resources` fixture dirs and `-manual.html` tests.
Without `--recursive`, `--root` still just globs `*.html` at that
directory's top level (the `dom/nodes` default's original, deliberately
non-recursive behavior — its own subdirectories are crashtests/other
never-vetted sub-suites, not part of the 168-file count).

(Omit `--binary` and it defaults to `target/$LUMEN_PROFILE/lumen.exe`; pass it
explicitly when running the script from a `git worktree`, whose own `target/`
is empty. Use `run_smoke.py` with an explicit test-id list instead only to run
an ad-hoc subset.)

## Adding a test / growing the suite

1. Pick a **synchronous** `test()`-based test (grep the candidate for
   `async_test`/`promise_test`/`test_driver`/`<iframe>`/`XMLHttpRequest` — skip
   if any match; those wait on machinery the BiDi-only executor doesn't drive
   yet). Confirm any `<script src=...>` helpers it pulls are vendored under
   `tests/wpt/` (a missing helper makes the test error, not fail cleanly).
2. Run it through `run_smoke.py` (above) with an **empty** `--metadata` dir to
   see the raw per-subtest result (all results show as "unexpected"), or read a
   `--log-wptreport=out.json` dump.
3. For each genuinely-failing subtest, add an `[<subtest name>]` /
   `expected: FAIL` block to `metadata/dom/nodes/<test>.html.ini` (escape `\`,
   `[`, `]` in the heading). File a `BUG-NNN` for the underlying engine gap and
   name it in the `.ini` header comment — **never edit the vendored test to
   make it pass.** A whole-test `TIMEOUT`/`ERROR` (harness never completed) is a
   deeper gap: prefer excluding the test and filing the bug over pinning
   `expected: TIMEOUT`.
4. Re-run the curated set and confirm it's still green (0 unexpected).

## Re-vendoring

See the "Re-vendoring" section of `tests/wpt/VENDOR.md`.
