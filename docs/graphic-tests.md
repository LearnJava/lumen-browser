# Graphic tests

`graphic_tests/NN-*.html` — 70+ pages (00 calibration + 01–81 unit properties + 100–109 interactions + `1000000-final.html`), one visual effect each, viewport 1024×720. Graphics only, no text.

**00-calibration.html** — required first test: magenta stripes (`#ff00ff`) 1024 px wide at top and bottom of body. Used to detect crop offset in the Lumen desktop screenshot.

---

## Magenta frame in all tests

Each test page 01+ uses a 1px magenta frame around the full 1024×720 viewport. Pattern:

```html
<style>
  body { background: #ff00ff; width: 1024px; height: 720px; }
  .__f { background: <PAGE_BG>; width: 1022px; height: 718px; margin: 1px; padding: <PADDING>; overflow: hidden; }
</style>
<body>
  <div class="__f">
    <!-- all content here -->
  </div>
</body>
```

The 1px magenta body background shows through `.__f`'s margins on all 4 sides. Crop offset comes from TEST-00 (calibration), not from this frame. Trigger phrases: "find bugs from screenshots", "run graphic_tests".

---

## Test layers (numbering ranges)

| Range | Layer | Purpose |
|---|---|---|
| `00–99` | Unit | One CSS property per file, isolated. A failure = bug in that property. |
| `100–199` | Interaction | Combinations of properties already covered by unit tests (transform×overflow, opacity×z-index, …). A failure while the unit deps are green = bug in the *interaction*. Deps map: `DEPS` in `run.py`. |
| `1000000` | Final | Kitchen-sink page; manual check in the Edge pipeline + CPU snapshot baseline. |

Interaction tests share a fixed 6-cell 300×300 grid (`_CELL_GRID` in `run.py`); on FAIL `run.py` intersects the diff bounding box with the cells and prints which scenario diverged. To diagnose a failing interaction test run `python graphic_tests/run.py --bisect <id>` — it runs the unit deps first, then the test, and prints a verdict (broken property vs broken interaction).

---

## Running

```bash
python graphic_tests/run.py                          # blocking pipeline: first fail = stop
python graphic_tests/run.py --only 03                # single test
python graphic_tests/run.py --continue-on-fail       # diagnostic: run all, collect all results
python graphic_tests/run.py --recheck                # re-run only FAIL tests from latest.json
python graphic_tests/run.py --build                  # cargo build --release first, then run
python graphic_tests/run.py --no-cache               # force re-capture Edge screenshots
python graphic_tests/run.py --bisect 100             # run unit deps of an interaction test, then the test; prints verdict
python graphic_tests/run.py --ipc                    # deterministic CPU snapshot over TCP (TAB-7); no window/gdigrab, no real JS
python graphic_tests/run.py --live                    # one live lumen window for the whole run (SDC-3); gdigrab capture, real JS
```

`--live` (SDC-3): keeps a single `lumen.exe --mcp-live-port N` window open for the entire run instead of killing and relaunching a process per test — the process-per-test model was the main source of wall-clock cost and focus-race flakiness ("magenta marker not found"). Navigation is driven through MCP (`LiveWindowSession`, SDC-2): `tools/call navigate` + `tools/call wait{condition:document_ready}` give a real load-complete signal instead of a blind `time.sleep`. The pixel capture itself is still gdigrab against the real femtovg-rendered window (not MCP's `resource://screenshot`, which renders through the CPU path and isn't at parity with femtovg — same gap as `--ipc`), so `--live` is safe for real-JS tests (57, 129–138) unlike `--ipc`. TEST-00 magenta calibration still runs once and the offset is reused for the rest of the run, same as the default mode.

Results are saved to `graphic_tests/results/`:
- `YYYYMMDD-HHMMSS.json` — full results: status, diff%, diff_region bounding box per test
- `YYYYMMDD-HHMMSS.html` — visual report: Edge | Lumen | Diff images side by side for each FAIL
- `latest.json` — always points to the last run (used by `--recheck`)

Edge screenshots are cached: re-captured only when the HTML source is newer than the PNG.

Pipeline: build Lumen release (if needed), then for each test — Edge headless + Lumen gdigrab + crop by magenta marker + pixel diff + % threshold. First test exceeding threshold stops the pipeline.

Output is one line per test:
```
TEST-03: PASS (0.2%)
TEST-07: FAIL (18.4%) ← pipeline stopped here
```

---

## Rule: adding a new CSS property

In the **same commit** as the implementation:

1. Add object(s) to the relevant test in series `02–20` (or create a new file if not covered).
2. Add a demo to `graphic_tests/1000000-final.html`.
3. Update `graphic_tests/COVERAGE.md` — add a row for the property.
4. If creating a new test file — use the magenta frame pattern: `body { background: #ff00ff; }` + `.__f` wrapper div with `margin: 1px; width: 1022px; height: 718px; background: <PAGE_BG>;`. See "Magenta frame in all tests" above.
5. Add an entry to `TESTS` in `graphic_tests/run.py`.

Current coverage — `graphic_tests/COVERAGE.md`.

---

## Run rules

0. **Test-run history lives in `graphic_tests/results/*.json`.** JSON result files are committed to git (`.gitignore` excludes only `*.html` reports). After every full `--continue-on-fail` run: `git add graphic_tests/results/<timestamp>.json && git commit -m "тесты: прогон YYYY-MM-DD"`. Do NOT write manual "Прогон..." tables in `BUGS.md` — the JSON is the source of truth. Delta vs previous run is printed automatically by `run.py`. KNOWN_DEBTORS (Phase 2 tests) live in `KNOWN_DEBTORS` dict in `run.py`; BUGS.md carries only BUG-NNN entries.
1. **No screenshots in the repo.** `graphic_tests/screenshots/*.png` are work artifacts — do not commit. HTML reports (`results/*.html`) are also gitignored (they reference gitignored PNGs). Only JSON results and [`BUGS.md`](../BUGS.md) go in.
2. **A bug is only a visually noticeable artifact.** Non-zero pixels in `<stem>-diff.png` alone are not a bug. Skip if only visible under pixel-by-pixel inspection.
3. **Ignore text for now.** Glyph antialiasing will always diverge from Edge — not tracked until a dedicated task. Text-box geometry, padding/margin around text, line-height — that's layout, check as normal.
4. **Never rewrite test pages to work around engine limitations.** Test pages are the ground truth — they represent correct CSS as Edge renders it. If a test fails, fix the engine, not the test. Simplifying HTML to make a test pass is a false positive: the engine didn't improve, the bar was lowered. The only valid reason to edit a test page is a bug in the test itself (wrong expected output).
4a. **Never change test thresholds. The diff threshold is 0.5% for every test** — in `graphic_tests/run.py` and `crates/driver/tests/snapshot_vs_edge.rs`. Raising a threshold to make a test pass is forbidden (user rule, 2026-06-11): it masks real defects. Precedent: BUG-093 was "closed" by calibrating TEST-51 to 2.0% — the actual cause was BUG-123, a scroll-clip defect eating the container's border. If a test exceeds 0.5%, file a BUG-NNN and fix the engine.
4b. **Known-debtors ratchet.** For pages that can't reach 0.5% because a feature is deferred: add `test_id → ("BUG-NNN", baseline_pct)` to `KNOWN_DEBTORS` in `run.py`. Ratchet (±2% gdigrab noise): regression if actual > baseline+2; ratchet down if actual < baseline-2; remove entry if ≤ 0.5%. Each entry requires an OPEN BUG-NNN. Full semantics in `run.py` comments and `docs/plans/cpu-vs-edge-gate.md`.
5. **Single tracker — `BUGS.md` in the repo root.** One line per bug, compact format:
   ```
   BUG-018 | OPEN  | inline padding wrong on nested divs | layout/src/flow.rs:312
   BUG-003 | FIXED 2026-05-10 | composite glyphs missing | font/src/parser.rs:201
   ```
   New bug: append with next number (current tail: BUG-022). Fixed: change `OPEN` → `FIXED <date>`, do not delete. WONTFIX: stays in file as-is.
