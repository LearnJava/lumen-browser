# Vendored Web Platform Tests — pin record

P2-wpt S2. Source: [`web-platform-tests/wpt`](https://github.com/web-platform-tests/wpt).

## Pinned commit

```
35be3b44f3111c4d614b5b201e399493d20e7b38
```

Committed 2026-07-13T16:23:05Z ("Cover a few fragment parsing edge cases (#61042)").
This was `master`'s tip at the time of vendoring — re-pin explicitly (do not silently
drift) when re-vendoring; record the new hash + date here.

## Vendoring method: committed snapshot, not a git submodule

A submodule would need `git submodule update` (a network fetch to GitHub) on every
fresh checkout — that violates this task's own "no live network in CI" rule
(`docs/tasks/p2-wpt-integration.md`, Prerequisites). A plain committed snapshot has
no such requirement: once vendored, the suite runs fully offline. The tradeoff is a
manual re-vendor step (re-run the process below) instead of `git submodule update`
to move the pin — acceptable since WPT is pulled in rarely and deliberately here.

## What's vendored (verbatim upstream source, not modified)

| Path (this repo) | Upstream path | Purpose |
|---|---|---|
| `tools/wptrunner/` | `tools/wptrunner/` | The test runner itself (CLI, executors incl. `webdriver-bidi`, manifest loading, expectations, formatters). |
| `tools/manifest/` | `tools/manifest/` | Test manifest generation/parsing — hard import of `wptrunner.testloader`/`wptrunner.metadata`. |
| `tools/serve/` | `tools/serve/` | The local HTTP/WS test server (`wptserve`-based) that serves this vendored test tree during a run — hard import of `wptrunner.environment`. |
| `tools/wptserve/` | `tools/wptserve/` | The HTTP/WS server library `tools/serve/serve.py` is built on. |
| `tools/webdriver/` | `tools/webdriver/` | WPT's own WebDriver/**WebDriver BiDi** Python client (`webdriver.bidi.client`, `webdriver.bidi.modules.browsing_context`, …) — this is what S3's `browsers/lumen.py` product plugin will drive against `lumen --bidi-port`. |
| `tools/metadata/` | `tools/metadata/` | `web-features` YAML schema — imported by `manifest.sourcefile`. |
| `tools/gitignore/` | `tools/gitignore/` | `.gitignore`-pattern matcher — imported by `manifest.vcs`. |
| `tools/localpaths.py` | `tools/localpaths.py` | Repo-root/sys.path bootstrap (`repo_root`, `sys.path` setup for the packages above). **Not modified** — see the sys.path note below. |
| `tests/wpt/resources/testharness.js` | `resources/testharness.js` | The real WPT client-side test harness (assertion/reporting primitives every `testharness.js`-style test imports). |
| `tests/wpt/dom/nodes/` | `dom/nodes/` | One full test category ("start tiny", per this task's Prerequisites) — includes the S4 smoke test, `Element-hasAttribute.html` (`Document-createElement.html`, floated as an "e.g." example when this file was first drafted, turned out to need un-vendored `/common/dummy.xml`/`dummy.xhtml` iframe fixtures and `async_test` — not actually trivial; picked a genuinely self-contained synchronous test instead). |
| `tests/wpt/FileAPI/` | `FileAPI/` | Second full test category, added 2026-07-21 by the WPT-VENDOR backlog (`ROADMAP.md` `WPT-VENDOR-FileAPI`, `docs/wpt-status.md`). Same pinned commit, `git sparse-checkout` at the same commit hash. A handful of its tests reference helper scripts outside `FileAPI/` (`/common/*.js`, `/resources/idlharness.js`, `/html/anonymous-iframe/resources/common.js`, `/service-workers/service-worker/resources/test-helpers.sub.js`) that were **not** vendored — those specific tests fail with a 404 for the missing helper, a documented survey gap in `docs/wpt-status.md`, not a Lumen engine bug. |

Each vendored top-level directory carries its own `LICENSE-WPT.md` (WPT is
3-clause BSD, copyright web-platform-tests contributors) alongside the code.

## What's explicitly NOT vendored, and why

- **`tools/third_party/`** (upstream is ~76 MB, dominated by vendored test corpora of
  its own — e.g. `third_party/hpack/test/` alone is ~55 MB of unrelated HTTP/2 header
  compression conformance fixtures). `tools/localpaths.py` unconditionally
  `sys.path.insert`s every `third_party/<pkg>` directory, but **a nonexistent
  `sys.path` entry is a silent no-op in Python** — it only matters if nothing else on
  `sys.path` provides the module. Every leaf package WPT vendors there
  (`pywebsocket3`, `h2`, `hpack`, `hyperframe`, `websockets`, `html5lib`,
  `webencodings`, `certifi`, `atomicwrites`, …) is also a normal, actively maintained
  PyPI package, verified installable and sufficient by actually importing the full
  chain (`localpaths` → `manifest.manifest` → `tools.serve.serve` →
  `wptrunner.wptrunner` → `wptrunner.wptcommandline` → `webdriver.bidi.client`) in a
  clean venv with only `pip install -r` (see `tests/wpt/README.md`) — no
  `tools/third_party` needed. This keeps the vendor commit small and avoids
  committing megabytes of an unrelated project's test fixtures.
- **`tools/wpt/`** (the `wpt` CLI wrapper script/package). Not imported by anything
  vendored above (`wptrunner` itself doesn't depend on it — it's the outer CLI, not
  the library); it also does venv bootstrapping and browser-download machinery this
  project doesn't need (we already have our own venv + a locally built `lumen.exe`).
  S4 calls `wptcommandline`/`wptrunner.run_tests` directly instead
  (`tests/wpt/run_smoke.py`) rather than vendor the whole wrapper for one flag's
  worth of behavior; S7 ("CI wrapper + docs") is where a polished wrapper — either
  growing `run_smoke.py` or actually vendoring `tools/wpt/` — gets decided.
- **Self-test suites of the supporting packages** (`tools/manifest/tests/`,
  `tools/wptserve/tests/`, `tools/serve/test_serve.py`,
  `tools/serve/test_functional.py`) — not imported by anything on the path we
  actually exercise (verified: the import-chain smoke check in
  `tests/wpt/README.md` passes without them), so dropped to keep the vendor
  commit to library code only. One of these (`tools/wptserve/tests/functional/
  docroot/test.asis`) also had a raw-HTTP-response fixture whose bytes are
  line-ending-sensitive — Git's LF normalization (`.gitattributes`) would have
  silently altered it, which is exactly the kind of "modified vendored code"
  this task rules out; not vendoring it sidesteps the problem rather than
  fighting it. **`tools/wptrunner/wptrunner/tests/`** is kept, unlike the
  above — S2 explicitly names `tools/wptrunner/` as vendored whole, it's small
  (186 KB), and it has no such fixtures.
- **`resources/testharnessreport.js`**: intentionally **ours to write**, not
  upstream's — see `docs/tasks/p2-wpt-integration.md` S4 (the per-product results
  shim; there is no generic upstream one to vendor — `tools/wptrunner`'s own
  `wptrunner/testharnessreport.js` is wptrunner's *default*, but S4 calls for a
  Lumen-specific `tests/wpt/resources/testharnessreport.js`).

## Re-vendoring

1. Pick a new upstream commit (or re-pin the same date to check for drift).
2. `git clone --filter=blob:none --sparse --depth 1 https://github.com/web-platform-tests/wpt.git`
   at that commit, `git sparse-checkout set` the paths in the table above.
3. Copy over the destinations in the table (verbatim — no diffing/patching upstream
   files; if Lumen needs different behavior, that's a fork decision to make
   explicitly, not a silent edit).
4. Re-run the import-chain smoke check from `tests/wpt/README.md` in a clean venv.
5. Update the pinned commit hash + date at the top of this file.
