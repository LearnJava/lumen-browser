# Web Platform Tests (WPT) — Lumen integration

P2-wpt (`docs/tasks/p2-wpt-integration.md`, slices S1–S8). Runs the real, unmodified
`wptrunner` against Lumen over WebDriver BiDi (`lumen --bidi-port N`) — not a
bespoke test runner. See the task doc for the full architecture and slice plan.

**Status:** S1–S4 done. `tests/wpt/run_smoke.py` drives a real `lumen --bidi-port`
through navigate + eval end to end and now reaches a genuine per-subtest PASS/FAIL
result (`dom/nodes/Element-hasAttribute.html`: 1/2 subtests PASS, 1 FAIL on a real,
separate engine gap — [BUG-297](../../bugs/BUG-297-OPEN.md), `setAttributeNS`
unimplemented). Getting here took five real engine/tooling gaps, fixed in order:
[BUG-278](../../bugs/BUG-278-FIXED.md) (HTTP client rejected `wptserve`'s
close-delimited responses), [BUG-279](../../bugs/BUG-279-FIXED.md)
(`document.getElementsByTagName` missing, broke `testharness.js`'s own module-level
setup), [BUG-280](../../bugs/BUG-280-FIXED.md) (`window` wasn't the JS engine's real
global object, so `testharness.js`'s `expose()`-based public API was unreachable),
[BUG-291](../../bugs/BUG-291-FIXED.md) (`Element`/`DocumentFragment`/
`ShadowRoot.querySelector(All)` weren't scoped to the calling node, so the results
renderer's detached-subtree build silently found nothing), and
[BUG-295](../../bugs/BUG-295-FIXED.md) (two independent causes stacked on one TIMEOUT
symptom: `wptrunner`'s vendored harness silently served its own generic
`testharnessreport.js` instead of Lumen's, and Lumen's persistent on-disk HTTP cache
replayed that wrong file across every subsequent run on `wptserve`'s fixed ports,
regardless of later fixes). See those bug files and the task doc's S4 section for the
full diagnosis trail (BiDi-eval-based bisection of `testharness.js`'s execution).

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

- `tests/wpt/run_smoke.py` — **ours** (S4) — minimal driver that calls
  `wptcommandline`/`wptrunner.run_tests` directly against the smoke test (see
  its own docstring for why this isn't `tools/wpt/wpt`). Run with:

  ```bash
  LUMEN_PROFILE=dev-release <venv>/python tests/wpt/run_smoke.py
  ```

  Both scripts default to `target/<LUMEN_PROFILE>/lumen.exe` (`LUMEN_PROFILE`
  env var, default `release`), same convention as `graphic_tests/run.py`.
  `run_smoke.py` currently exits non-zero — see Status above (the one real subtest
  FAIL, BUG-297, has no `.ini` expectation yet, so it counts as "unexpected").
- `tests/wpt/config.json` — **ours** (S4) — `wptserve` config override: pins
  `browser_host` to `127.0.0.1` (the default, `web-platform.test`, needs
  `/etc/hosts` entries this task's "no live network" rule can't rely on) and
  disables the `wss`/`h2`/`webtransport-h3`/`dns` servers the smoke test
  doesn't need (Python 3.14's `ssl` module dropped `wrap_socket`, breaking
  `wptserve`'s `wss` server; unrelated to Lumen).
- `tests/wpt/metadata/` — `--metadata` root; holds the generated (gitignored)
  `MANIFEST.json` and will hold `.ini` expectations from S5 onward.

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

## Adding a test / growing the suite

Not yet applicable — S5 ("Expectations + curated subset") is where the included
test set grows past the single vendored `dom/nodes/` category and `.ini`
expectations get introduced. This section will be filled in then.

## Re-vendoring

See the "Re-vendoring" section of `tests/wpt/VENDOR.md`.
