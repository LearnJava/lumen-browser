# CLAUDE.md

Project context for Claude Code. Auto-loaded each session. Keeps the assistant oriented without re-asking questions answerable from code or adjacent docs.

**This file is English-only.** All edits — including gotchas added by other sessions — must be written in English. Translate before committing.

Update this file whenever you change architecture, invariants, or policies.

---

## What is this

**Lumen** — private, lightweight, transparent browser in Rust with a custom engine. Not a Chromium/WebKit wrapper; a standalone rendering engine with an embedded JS engine.

Current phase: **Phase 2 — v0.5 «Interactive» (complete)**, app version **v0.5.0**. Phase 0 (prototype) closed 2026-05-26; Phase 1 «Reader» largely complete. Phase 2 delivered: QuickJS, Canvas 2D, CSS Grid, Shadow DOM, accessibility tree, forms, find-in-page, DevTools/CDP, knowledge layer.

**JS engine: V8 (`rusty_v8`) is the DEFAULT since the S12 cutover (ADR-018, 2026-07-14).** QuickJS/`rquickjs` remains only as an opt-in rollback path (`--features quickjs`) and is being deleted slice-by-slice (S12b, `docs/tasks/ph3-v8-migration.md`). Never target new functionality, fixes, or investigation at the rquickjs path; the engine-agnostic JS shim (`WEB_API_SHIM` in `crates/js/src/dom.rs`) is shared by both engines and is the right place for engine-independent fixes. Validate JS work against the default (V8) build.

### Versioning & phase policy

Single source of truth for the version is `[workspace.package] version` in `Cargo.toml`. All machine-readable version strings (User-Agent, Sec-CH-UA, CDP `Browser.getVersion`, window title, startup banner) derive from `CARGO_PKG_VERSION` — do **not** hardcode a version number in code. The one manual-bump site is the `navigator.userAgent` literal in `crates/js/src/dom.rs` (JS shim string).

Version↔phase mapping (from `docs/plan/phases.md`): Phase 1 → v0.1, **Phase 2 → v0.5** (target on phase completion), Phase 3 → v1.0. Mid-phase the version climbs toward the target (Phase 2 reached its **0.5.0** target). Reaching Phase 3 → 1.0.0. Keep the phase label in sync across `README.md`, `docs/plan/phases.md`, this file, and the shell startup banner.

| File | Contents |
|---|---|
| `CAPABILITIES.md` | **Source of truth for "what the browser can do right now"** (per-subsystem, ✅/🟡/⬜, verified against code). Read ONLY this for capability questions — not `docs/plan/*` or `STATUS-PN.md`. Update in the same commit as a feature merge. |
| `README.md` | User-facing: install, commands, what to expect. |
| `STATUS-PN.md` | **Bare pointer lines `<source>:NN` and nothing else** — one line per open task, priority top→bottom, no headers/prose/completed tasks (schema: `docs/dev-roles.md` §Task tracking schema). `<source>` = ROADMAP.md (P1/P2) · BUGS.md (P3) · CSS-SPECS.md (P4) · a code `file:line` for a `// CSS:` / `// BUG-NNN` handoff. Read yours at session start. Detail belongs in the source row, `docs/tasks/<id>.md`, or `bugs/BUG-NNN-*.md` — never here. Exception: `STATUS-P5.md`, whose source is a health sweep rather than a row list (alias→action table, format still provisional). |
| `lumen-plan.md` | TOC index: links to 11 section files in `docs/plan/`. Read for architecture; for daily status use `STATUS-PN.md` instead. |
| `docs/plan/` | Design doc split into 11 files: architecture, tech-stack, engine, web-apis-shell, privacy, features, knowledge, security-performance, testing, phases, meta. (The former `roadmap.md`/`history.md` were deleted 2026-07-02 — task status lives in `ROADMAP.md`, chronology in `git log`.) |
| `CSS-SPECS.md` | Complete CSS property & spec roadmap: all W3C modules, per-property status (✅🟡⬜🚫), P4 priority queue. |
| `docs/wpt-status.md` | WPT readiness: all 277 upstream top-level categories (scope ⬜/🚫, vendored status), plus a per-test detail table for the one vendored category (`dom/nodes`, 168 tests) with pass/fail and an assignable Владелец/Баг column. Regenerate the detail table with `tests/wpt/gen_status_md.py` after a fresh `run_report.py --all` run — read the file's own "Как обновить" section, not this line, for the exact commands. |
| `docs/build-speed.md` | Compile-time optimization plan: current baseline, measurement protocol (S1–S5), ranked measures (stable / nightly / rejected), benchmark journal. Read before changing build config (profiles, `.cargo/config.toml`, sccache). |
| `docs/perf-method.md` | **How to measure, gate and accept a performance change** (census before the fix, profile the path you change, gate by counter/identity not wall-clock, interleaved A/B compared on min, instrumentation traps). Distilled from the 27 slices of BUG-341. Read before a perf task; the numbers themselves stay in `bugs/BUG-NNN-*.md`. |
| `docs/automation.md` | **All automation/introspection surfaces of the browser and when to apply them** (dump modes, `--deterministic`, MCP tools/resources, BiDi, IPC, driver-API, `LUMEN_NO_*` paint-bisect flags, known stubs). Read before writing a debugging script or a new test harness — the capability usually already exists. |
| `docs/roadmap-trees.md` | **How to use the interactive roadmap trees** (`docs/roadmap-*.html`): open in a browser, filters/search, and how to keep them current (`ROADMAP.md` + `python scripts/gen_roadmap.py`, auto-pulls bug status from `BUGS.md`). |
| `ROADMAP.md` | Flat, grep-friendly source of the phase/task tree (two markdown tables: phases + tasks, one task per line). Feeds `gen_roadmap.py`; replaced the old nested `docs/roadmap.json`. Bug↔task links live in its `bugs` column; CSS-module status is live-aggregated from `CSS-SPECS.md` into rows `css-specs-t0`…`t4` (note = `AUTO:CSS-SPECS:T<N>`, do not hand-edit that note). |
| `CLAUDE.md` | (this file) Conventions and invariants for the assistant. |
| `REVIEW.md` | **Code-review policy** — what a reviewer blocks a change on (hard gates), architecture boundaries, style/design rules, docs that must move in the same commit, and what is explicitly out of scope for review. Read before reviewing someone's diff; the rules restate this file and `docs/conventions.md` in reviewer form, so on conflict trust the code and flag the change. |
| `docs/decisions/` | Formal ADR files (one per architectural decision). See README.md + TEMPLATE.md inside. |
| `DECISIONS.md` | Historical decisions (pre-ADR format). Read-only — add new decisions to `docs/decisions/` instead. |
| `samples/page.html` | Test page for pipeline runs. |
| `assets/fonts/Inter-Regular.ttf` | Bundled font (SIL OFL 1.1). |

---

## Working boundary

**Write code only inside the browser folder** — `D:\RustProjects\lumen-browser\` and its worktree copies in `.claude/worktrees/*`. Same applies to docs, configs, snapshot tests. Everything outside — `~/.bashrc`, `~/.config/*`, system dotfiles, sibling projects, **ad-hoc worktrees like `../lumen-<task>/`** — do not touch. If a task requires external changes, describe what the user should do and wait for approval.

`git worktree add` follows the same rule: path must be `.claude/worktrees/<task-name>/` (inside the browser folder), **not** `../lumen-<task>/` or anywhere outside.

Exception: Claude memory (`~/.claude/projects/.../memory/`) lives outside the repo by design — the boundary rule does not apply to it.

---

## Developer assignments

Full role definitions, workflows, collaboration rules, task tracking schema — [`docs/dev-roles.md`](docs/dev-roles.md).

**If the user says "you are developer N" at session start — read `STATUS-PN.md` and take the first pointer line. If a `p<N>-…` branch already exists for you (`git branch`), continue that task instead. If all your tasks are taken — ask the user which task to take next.**

**Take the list strictly top-down — never skip a line because a lower one looks smaller, clearer or more fun.** The order is not a preference list: it encodes dependencies. A lower line is often the *symptom* of a higher one (e.g. BUG-425 «sidebar items on one line» is caused by BUG-333 `height: var()` → `h=0`), so working out of order means fixing a symptom on top of a live defect and re-doing it later. If the first line genuinely cannot be started (blocked by another role, needs a user decision), say so explicitly and ask — do not silently move down the list.

| Developer | Domain | Crates |
|---|---|---|
| **P1** | General feature development (source → layout → paint → shell), taken top-down off `STATUS-P1.md`. Finished tracks: DS (design system v3.3, DS-1…DS-19, `docs/design/lumen-v3_3.html`). Current: the CC track (engine chrome) — `STATUS-P1.md` is ordered as one dependency chain «engine roots → chrome visuals → chrome interaction», engine-root bugs that only the chrome exposes (BUG-333/433/431/343/288) were moved here from P3 on 2026-07-29 so one role owns the whole chain. BUG-341 (incremental restyle) is paused by user decision 2026-07-28 — resume only on explicit request | All crates (coordinated with P2/P4) |
| **P2** | **Reactivated 2026-07-13**: leads P2-wpt (WPT via `wptrunner` + WebDriver BiDi, `docs/tasks/p2-wpt-integration.md`) and the DEVX track (dev-tooling on existing automation surfaces, `docs/automation.md`, ROADMAP.md DEVX-1…6, assigned 2026-07-16). Was reserve (since 2026-06-18). | `lumen-bidi-server`, `lumen-driver`/`lumen-mcp` (DEVX-5), Python tooling `tests/wpt/` + `graphic_tests/run.py` (DEVX-1/4) |
| **P3** | **Bug fixes ONLY**: BUGS.md OPEN items, graphic test regressions | All crates (read-only except bug fixes) |
| **P4** | **CSS properties ONLY**: parsing, ComputedStyle, cascade, end-to-end wiring | `css-parser`, `layout` (style.rs), `paint` (display_list.rs) |
| **P5** | **Code health ONLY**: audit, workspace-clippy, stub/branch/docs/dep sweeps, safe mechanical cleanup | All crates (read-only except trivial clippy fixes in own crate + branch/worktree/SYMBOLS.md cleanup) |

**Task reservation:** create the `p<N>-<id>` branch — its existence is the reservation signal. A parallel session sees it via `git branch` and skips the task. Details — `docs/dev-roles.md`.

---

## Project Skills

6 skills in `.claude/skills/`. Use them instead of following protocols manually:

| Skill | When to use |
|---|---|
| `/lumen-add-css-property` | Adding a new CSS property to `lumen-layout` |
| `/lumen-task-start <name>` | Starting a new roadmap task (creates worktree + reserves in plan) |
| `/lumen-task-finish <name>` | Task ready to merge (clippy → tests → merge --no-ff → worktree remove) |
| `/lumen-new-crate <name>` | Creating a new Cargo crate in the workspace |
| `/lumen-health-check [target]` | P5 maintenance sweep (`full`/`clippy`/`stubs`/`branches`/`docs`/`deps`/`dupes`) |
| `/lumen-perf-audit` | Real-site performance audit (PERF track): run the corpus, collect per-phase stats, diff vs previous run, journal + file bugs |

`lumen-task-start` — explicit invocation only (`/`).
`lumen-add-css-property`, `lumen-new-crate`, `lumen-health-check`, `lumen-perf-audit`, and `lumen-task-finish` — Claude may invoke automatically from context.

---

## Commands

Full reference (token efficiency, OS detection, PATH setup) — [`docs/commands.md`](docs/commands.md).
Automation & diagnostics (dumps, deterministic mode, MCP/BiDi/IPC drive, paint-bisect env flags) — [`docs/automation.md`](docs/automation.md).

```bash
export PATH="/c/Users/konstantin/.cargo/bin:$PATH"          # Git Bash only

cargo check -p lumen-layout                                  # fast check, 1-2s
cargo clippy -p lumen-layout --all-targets -- -D warnings   # required before commit
cargo test -p lumen-font                                     # crate tests
cargo run -p lumen-shell -- samples/page.html               # run with test page
cargo run -p lumen-shell -- --dump-layout samples/page.html # headless layout dump
cargo run -p lumen-shell -- --dump-display-list samples/page.html  # headless paint dump
```

**Session start protocol:**
1. **`git pull origin main` first, before reading STATUS files or creating a branch/worktree.** Parallel Claude Code sessions push to `origin/main` independently, so local `main` can lag behind — starting from stale state means stale `STATUS-PN.md`/`ROADMAP.md` reads and, if the lag is large, a big multi-file merge conflict later instead of a small one now. If `pull` reports a diverged history with real conflicts (not just a fast-forward), resolve them file-by-file with full understanding of both sides' intent — do not blindly take one side — and re-verify with `cargo check`/`clippy`/scoped tests for every touched crate before committing the merge.
2. Read `STATUS-PN.md` — pointer lines to open tasks; `git branch` shows any `p<N>-…` task in progress
3. Run `git branch` — verify you're on main
4. Architecture context → `docs/plan/architecture.md` §1, §3; decisions → `docs/decisions/README.md`

**Session end: push what you finished.** A completed task's merge commit (7-step checklist below, step 5) must be pushed to `origin/main` immediately, not left sitting local — other parallel sessions rely on seeing it to avoid duplicate/conflicting work on the same area.

**Cargo output rules:** always `-p <crate>`, never `--workspace` (exception: P5). Success → 1 line. Errors → full `error[...]` block, skip all warnings. Test failure → test name + first 10 lines.

**Run discipline (details in `docs/commands.md`):** one cargo run — one log file (`> .tmp/<name>.log 2>&1`, then grep the file; never re-run cargo just to re-filter output). During iteration `cargo check -p` only; one `clippy -p` + targeted tests before the commit; full gates (workspace clippy + scoped-test) run exactly once inside `/lumen-task-finish`, synchronously in the foreground — never as background tasks. Don't hand-run expensive crate tests before the gate — `scoped-test.sh` repeats them.

**Long runs you are not blocked on** (baseline builds, `test --no-run`, the graphic pipeline) go to the background via the Bash tool's `run_in_background: true` — the harness notifies you on exit, so keep working meanwhile. Never `cmd &` **plus** `run_in_background` (silently kills the process) and never poll with `sleep N` — that pattern burned 76 of 188 minutes on 2026-07-27. Foreground-only exceptions: the final gates, and the full graphic pipeline (gdigrab needs a focused window; backgrounded, TEST-00 fails "magenta marker not found" and cascades).

---

## Graphic tests

Full documentation (magenta frame pattern, test layers, run flags, KNOWN_DEBTORS, run rules) — [`docs/graphic-tests.md`](docs/graphic-tests.md).

`graphic_tests/NN-*.html` — 70+ pages, viewport 1024×720. Graphics only, no text.

```bash
python graphic_tests/run.py --continue-on-fail   # run all, collect results
python graphic_tests/run.py --only 03            # single test
python graphic_tests/run.py --bisect 100         # diagnose interaction test
```

**Adding a new CSS property** (same commit as implementation):
1. Add to relevant test in `02–20` (or new file with magenta frame pattern)
2. Add demo to `graphic_tests/1000000-final.html`
3. Update `graphic_tests/COVERAGE.md`
4. Add entry to `TESTS` in `graphic_tests/run.py`
5. **If the property affects paint/rasterization**, regenerate the deterministic CPU snapshot references in the **same commit**: `SAVE_CPU_SNAPSHOTS=1 cargo test -p lumen-driver --features cpu-render cases::snapshot_cpu` (then review the changed PNGs are correct, not garbage). Skipping this drifts `graphic_tests/snapshots/cpu/` on unrelated pages and later red-lights the `scoped-test.sh` gate for someone else — the recurring BUG-118 / BUG-149 / BUG-297 / BUG-316 staleness.

**When the full ~20-min run is required:** anything that can move pixels — paint/display list, layout geometry, a CSS property, font/text/image. For changes that cannot alter the display list (pure perf, refactors, tooling, docs) the gate is `scripts/scoped-test.sh` (includes the deterministic CPU snapshots) + `python graphic_tests/dump_golden.py`; claiming display-list neutrality requires showing an empty `dump_golden.py` diff. Details — [`docs/graphic-tests.md`](docs/graphic-tests.md).

**Hard rules:** never edit test pages to work around engine limits; never change thresholds (0.5% for all); no screenshots committed.

---

## Architecture

Dependency graph and crate scope — in [`docs/plan/architecture.md`](docs/plan/architecture.md) §3. Direction: `lumen-core` → dom/font/parsers → layout → paint → shell. No cycles.

### Extension traits (`lumen-core::ext`)

Full list with implementations — [`subsystems/core.md`](subsystems/core.md). Planned: `WindowingBackend`, `RenderBackend`, `TlsBackend`.

---

## Principles

Full list (8 items) — [docs/plan/architecture.md](docs/plan/architecture.md) §1.

---

## Dependency policy

Full tables (permanent + provisional + Lumen core) — [docs/plan/tech-stack.md](docs/plan/tech-stack.md) §5.

### No new dep without justification

Every new `[dependencies]` entry requires this in the commit body:

> **Why this dependency:** \<category (permanent / provisional), trait-anchor, graduation criterion if provisional\>

---

## Code conventions

Full details (style, tests, error handling, unsafe) — [`docs/conventions.md`](docs/conventions.md).

- **Rust 1.95+ stable**, Edition 2024, resolver "3", MSVC on Windows.
- `cargo clippy -p <crate> --all-targets -- -D warnings` must pass before every commit.
- **`///` doc comments on all public structs, fields, and functions** — mandatory.
- No `panic!` / `unwrap()` in production code; allowed in tests.
- `unsafe` forbidden outside FFI boundaries; every block requires `// SAFETY:` comment.
- Names: `snake_case` functions/fields, `PascalCase` types, `SCREAMING_SNAKE` constants.

---

## Git workflow

Full protocol (commits, worktree isolation, zombie worktree, 7-step checklist) — [`docs/git-workflow.md`](docs/git-workflow.md).

**All work happens in feature branches. Direct commits to `main` are forbidden.**

Branch naming: `p<N>-<task-name>` (P1–P5 prefix mandatory). `--no-ff` required on merge. Commit message in Russian, subject under 80 chars, body explains *why*.

**Forbidden:** direct commit to main · force-push · rewriting history · `git config` · `--no-verify` · `git push` without explicit user request.

**Every session MUST work in its own `git worktree`** — use your **persistent pool slot**, one per developer, instead of creating a worktree per task:

```bash
cd "$(bash scripts/worktree-pool.sh p<N>-work p<N>-task-name | tail -1)"   # occupy
bash scripts/worktree-pool.sh list                                        # who holds what
bash scripts/worktree-pool.sh release p<N>-work                           # free (step 3 of the checklist)
```

The slot (`.claude/worktrees/p1-work` … `p5-work`, plus `perf-base`) is created once and reused — only the branch changes, so `target/` stays warm and rebuilds are incremental. `git worktree remove` after every task deleted that cache and cost 9–15 min of cold build per task, plus 3 min for the `add` itself. Build **only `dev-release`** inside a slot (warm `dev-release` ≈ 4.7 GB; five slots on `debug` would be ~70 GB). The script refuses to switch a slot with uncommitted or unmerged work. Details — [`docs/git-workflow.md`](docs/git-workflow.md).

**7-step completion checklist** (all mandatory, full details in `docs/git-workflow.md`):
1. `cargo clippy -p <crate> -- -D warnings` + `cargo test -p <crate>`
2. `git merge --no-ff p<N>-task-name -m "Merge …"`
3. `bash scripts/worktree-pool.sh release p<N>-work` then `git branch -d p<N>-task-name` (a slot holding the branch makes `branch -d` fail)
4. Delete pointer line from `STATUS-PN.md`, commit
5. `git push origin main`
6. Pool slot — nothing to remove (freed in step 3). Ad-hoc worktree — `git worktree remove .claude/worktrees/<task-name>`

---

## Communication

- **Reply language: Russian.** The user speaks Russian.
- **Tone: technical, no emoji** unless the user uses them.
- **Brief and direct.** Short answer + what was done. No marketing text.
- **Files as clickable links:** `[lumen-plan.md](lumen-plan.md)`, `[crates/engine/font/src/rasterizer.rs:48](crates/engine/font/src/rasterizer.rs)`.

### Banned words

"Wikipedia" / "Википедия" — user explicitly asked not to use. Say "reference article", "external article", "external page" instead.

---

## Doc sync rules — update matrix

Full rules (what NOT to update, what needs no update) — [`docs/doc-sync.md`](docs/doc-sync.md).

Update docs **in the same commit** as the code change. Use `grep -n` to find the line, then targeted `Read offset=N limit=10` + `Edit`.

| Change type | Files to update | What exactly to do |
|---|---|---|
| New feature / capability | `CAPABILITIES.md` + `subsystems/<crate>.md` | ⬜/🟡 → ✅; append bullet to Done section |
| New feature / capability | `STATUS-PN.md` | delete completed task's pointer line |
| Bug fixed | `BUGS.md` | `OPEN` → `FIXED <date>` |
| CSS property (P4) | `CSS-SPECS.md` + `CAPABILITIES.md` | ⬜ → ✅ |
| New dependency | `docs/plan/tech-stack.md` | append row |
| Architectural decision | `docs/decisions/ADR-NNN.md` | new file from TEMPLATE.md; update index |
| Known gotcha found/fixed | `CLAUDE.md` → "Known gotchas" | append/remove bullet |
| Perf slice merged with a transferable lesson | `docs/perf-method.md` | append the rule (one paragraph, slice ref in brackets); per-slice numbers stay in `bugs/BUG-NNN-*.md` |
| New public API | `SYMBOLS.md` | `python scripts/gen_symbols.py` |
| Roadmap/bug/CSS-module status change | `ROADMAP.md` → `python scripts/gen_roadmap.py` | edit ROADMAP.md if structure changed; CSS-module status alone needs no edit — the script re-pulls it from CSS-SPECS.md |

---

## Subsystem state

Per-crate state (scope, done, deferred, invariants) — [SUBSYSTEMS.md](SUBSYSTEMS.md) (index) → `subsystems/<crate>.md`. Update the relevant crate file on every plan-item commit.

---

## Decisions log

**New decisions** — one ADR file per decision in [`docs/decisions/`](docs/decisions/), using the template at [`docs/decisions/TEMPLATE.md`](docs/decisions/TEMPLATE.md). Update the index table in [`docs/decisions/README.md`](docs/decisions/README.md).

**Historical decisions** (pre-ADR format) — [`DECISIONS.md`](DECISIONS.md). Do not add new entries there.

---

## Unique features (§12)

Full list with phases — [docs/plan/knowledge.md](docs/plan/knowledge.md) §12.

---

## Known gotchas

- **Cargo.lock is committed** (workspace includes a binary).
- **Line endings:** `.gitattributes` enforces LF. Git warning about CRLF→LF is normal.
- **Archives in repo root are gitignored** (`/*.zip`, `/*.tar*`). Downloaded files won't accidentally get committed.
- **Portable user data dir (`<exe_dir>/data/`).** The ad-block external-filter-list subsystem stores its data under `<exe_dir>/data/adblock/` (SQLite `adblock.db` for subscriptions + list metadata; `lists/<slug>.txt` bodies; `custom-rules.txt`) — see `shell/src/adblock.rs::browser_data_dir`. This is a **provisional** convention (user decision 2026-06-16): keep everything in the browser folder, do **not** use OS dirs (`%APPDATA%`/`~/.config`/`~/.cache`) or `lumen_cache_dir()`/`config_path()` for portable data. New subsystems needing portable data should add their own `data/<subsystem>/` subfolder via `browser_data_dir()`.
- **`gdigrab` captures the whole desktop, not just the Lumen window — any OS focus change during a `graphic_tests/run.py` run (live-window default, SDC-4) silently corrupts every screenshot from that point on.** Found 2026-07-28 while validating SDC-4: a live-window full run showed 72/149 tests FAIL at 85–93% diff, all starting from one point onward; the "Lumen" screenshots turned out to be pictures of the Claude Code chat window itself — the OS foreground had shifted there (a chat message arrived mid-run) and `_bring_pid_to_front`'s alt-key trick never reclaimed it for the rest of the run. Not a regression from SDC-4 — the same `gdigrab`+bring-to-front mechanism was already used by the old per-test-process default, so the risk is inherent to the whole pipeline, not new. Unlike the backgrounded case (which fails loudly at TEST-00, "magenta marker not found"), a mid-run focus steal fails silently as a wall of unrelated-looking high-diff FAILs — don't diagnose those as engine regressions before checking whether anything touched the desktop (including typing into this chat) while the run was in flight. Re-run clean (no desktop interaction) before trusting a graphic-tests result with many simultaneous large-diff failures.
- **A top-level `/resources/testdriver.js` (repo root, sibling of `tests/`/`crates/`, not under `tests/wpt/`) is not clutter — do not delete or relocate it.** `tools/wptrunner/wptrunner/environment.py::get_routes` hardcodes the `/resources/testdriver.js` static route as `[<repo_root>/resources/testdriver.js, executors/message-queue.js, testdriver-extra.js]` concatenated, where `repo_root` is computed by walking three `os.pardir`s up from `environment.py`'s own path. In upstream wpt that lands on the wpt checkout root (where `resources/testdriver.js` really lives); in this repo, where `tools/wptrunner` is vendored under `tools/` instead of at the repo root, the same walk lands on the *Lumen* repo root instead — an accident of vendoring depth, not a deliberate layout choice. Not overridable via `env_options()` the way `testharnessreport` is (no such hook on this route) and `tools/wptrunner` is not patched (unmodified-vendor rule), so this is the only place the file can live for `test_driver.*` support (WPT-RUN-2) to work at all — see `tests/wpt/VENDOR.md`.
- **Parallel sessions in the same working tree = disaster.** Two sessions doing `git checkout` of different branches causes git to stash one session's work. Recovery via `git stash pop` is fragile. **Solution: mandatory `git worktree`s** (see Worktree isolation above). If you find yourself on a foreign branch — check `git stash list` before running `git restore .`.
- **~~Live-window BiDi/MCP `script.evaluate` can hang indefinitely~~ — misdiagnosis, root-caused 2026-07-17 (P2-wpt S4).** The "`script.evaluate` reports `JS context not available` forever" symptom previously attributed to an environment-dependent JS-runtime-install race was actually three separate, fully deterministic engine/shell bugs, none of them environment-flaky: [BUG-298](bugs/BUG-298-FIXED.md) (`Element`/`DocumentFragment`/`ShadowRoot`.querySelector(All) searched the whole document instead of the calling node's subtree, so any code building a detached DOM subtree and querying into it — e.g. `testharness.js`'s own results-table renderer — silently got nothing), [BUG-299](bugs/BUG-299-FIXED.md) (`Element.prototype.insertAdjacentText` was missing entirely, thrown from the same code path), and [BUG-300](bugs/BUG-300-FIXED.md) (`browsingContext.navigate`'s `DocumentReady` wait could ACK using the *previous* page's stale `layout_box` before the new page had even started loading). All three are now fixed. The `run_smoke.py`-only follow-up timeout was [BUG-301](bugs/BUG-301-FIXED.md), **now fixed too (2026-07-18)**: `wptrunner` registers a static route for `/resources/testharnessreport.js` serving its *own* `__wptrunner_message_queue` report, which wins over the on-disk file — so Lumen's vendored report (the one that sets `window.__lumen_wpt_results`, the global `LumenTestharnessExecutor` polls) was never served under `wptrunner`+`wptserve`, hence "works manually over a plain HTTP server, times out under wptrunner". Fixed by `browsers/lumen.py::env_options` pointing `testharnessreport` at Lumen's own report file. `--bidi-port`/`--mcp-live-port` session-restore racing (a separate, unrelated mechanism) was already fixed by [BUG-296](bugs/BUG-296-FIXED.md). Gotcha when re-checking WPT: a **stale `target/dev-release/lumen.exe`** will mimic an unrelated empty-subresource-fetch bug — rebuild before trusting a `run_smoke.py` failure.

When you discover a non-obvious implementation detail in a specific subsystem, add it to [`subsystems/<crate>.md`](subsystems/) under the relevant crate section (in English), not here.

---

## When in doubt

- **Architecture / scope** — `docs/plan/architecture.md` (§1 Principles, §3 Architecture).
- **Dependency policy** — `docs/plan/tech-stack.md` (§5).
- **How to build / run** — `README.md`.
- **Current code state** — `git log --oneline`.
- **Why a decision was made** — `docs/decisions/ADR-*.md` or `DECISIONS.md` (historical).

If the question isn't answered by these sources — ask the user, don't assume.
