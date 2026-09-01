# CLAUDE.md

Project context for Claude Code. Auto-loaded each session. Keeps the assistant oriented without re-asking questions answerable from code or adjacent docs.

**This file is English-only.** All edits — including gotchas added by other sessions — must be written in English. Translate before committing.

Update this file whenever you change architecture, invariants, or policies.

---

## What is this

**Lumen** — private, lightweight, transparent browser in Rust with a custom engine. Not a Chromium/WebKit wrapper; a standalone rendering engine with an embedded JS engine.

Current phase: **Phase 2 — v0.5 «Interactive» (complete)**, app version **v0.5.0**. Phase 0 (prototype) closed 2026-05-26; Phase 1 «Reader» largely complete. Phase 2 delivered: QuickJS, Canvas 2D, CSS Grid, Shadow DOM, accessibility tree, forms, find-in-page, DevTools/CDP, knowledge layer.

**JS engine: V8 (`rusty_v8`) is the ONLY JS engine, full stop.** S12b-F1 (2026-08-04) removed the `quickjs` shell rollback feature; `QuickJsRuntime` itself was deleted in S12b-F2 (2026-08-04); `dom.rs::install_primitives` (the rquickjs native-registration entry point, 2736 lines) was deleted in S12b-F3 (2026-08-04); S12b-F4 (2026-08-04, closing the `P3-v8-s12b` track) removed the `rquickjs`/`rquickjs-core`/`rquickjs-sys` dependency from `crates/js/Cargo.toml` and `Cargo.lock` outright — `rquickjs` is gone from the workspace entirely, not just unused. Never target new functionality, fixes, or investigation at the rquickjs path (it no longer exists); the JS shim (the `WEB_API_SHIM*` consts in `crates/js/src/dom.rs`, `#[cfg(feature = "v8-backend")]`) is evaluated only by the V8 install path (`v8_runtime.rs::install_dom`) and is the right place for engine-independent fixes. **Since SPLIT-JS3 (2026-08-28) the shim's text is not in `dom.rs`** — the 14 consts are `include_str!("shim/<name>.js")` and the JS lives in `crates/js/src/shim/*.js`, one file per const, so edit the `.js` file and grep there rather than in `dom.rs`. Those files are read verbatim: nothing in them is escaped, and adding a `\"` corrupts the JS instead of protecting it. Validate JS work against the default (V8) build.

### Versioning & phase policy

Single source of truth for the version is `[workspace.package] version` in `Cargo.toml`. All machine-readable version strings (User-Agent, Sec-CH-UA, CDP `Browser.getVersion`, window title, startup banner) derive from `CARGO_PKG_VERSION` — do **not** hardcode a version number in code. The one manual-bump site is the `navigator.userAgent` literal in `crates/js/src/shim/web_api_shim_mid_b.js` (it was in `crates/js/src/dom.rs` until SPLIT-JS3 moved the shim text out, 2026-08-28).

Version↔phase mapping (from `docs/plan/phases.md`): Phase 1 → v0.1, **Phase 2 → v0.5** (target on phase completion), Phase 3 → v1.0. Mid-phase the version climbs toward the target (Phase 2 reached its **0.5.0** target). Reaching Phase 3 → 1.0.0. Keep the phase label in sync across `README.md`, `docs/plan/phases.md`, this file, and the shell startup banner.

| File | Contents |
|---|---|
| `CAPABILITIES.md` | **Source of truth for "what the browser can do right now"** (per-subsystem, ✅/🟡/⬜, verified against code). Read ONLY this for capability questions — not `docs/plan/*` or `STATUS-PN.md`. Update in the same commit as a feature merge. |
| `README.md` | User-facing: install, commands, what to expect. |
| `STATUS-PN.md` | **Bare pointer lines `<source>:NN` and nothing else** — one line per open task, priority top→bottom, no headers/prose/completed tasks (schema: `docs/dev-roles.md` §Task tracking schema). `<source>` = ROADMAP.md (P1/P2) · BUGS.md (P3) · CSS-SPECS.md (P4) · a code `file:line` for a `// CSS:` / `// BUG-NNN` handoff. Read yours at session start. Detail belongs in the source row, `docs/tasks/<id>.md`, or `bugs/BUG-NNN-*.md` — never here. Exception: `STATUS-P5.md`, whose source is a health sweep rather than a row list (alias→action table, format still provisional). |
| `lumen-plan.md` | TOC index: links to 11 section files in `docs/plan/`. Read for architecture; for daily status use `STATUS-PN.md` instead. |
| `docs/plan/` | Design doc split into 11 files: architecture, tech-stack, engine, web-apis-shell, privacy, features, knowledge, security-performance, testing, phases, meta. (The former `roadmap.md`/`history.md` were deleted 2026-07-02 — task status lives in `ROADMAP.md`, chronology in `git log`.) |
| `CSS-SPECS.md` | Complete CSS property & spec roadmap: all W3C modules, per-property status (✅🟡⬜🚫), P4 priority queue. |
| `docs/wpt-status.md` | WPT readiness: all 277 upstream top-level categories (scope ⬜/🚫, vendored status), plus a per-test detail table for the one vendored category (`dom/nodes`, 168 tests) with pass/fail and an assignable Владелец/Баг column. Regenerate the detail table with `tests/wpt/gen_status_md.py` after a fresh `run_report.py --all` run — read the file's own "Как обновить" section, not this line, for the exact commands. The category index's "Заметка" cell must stay a one-sentence summary + link — full per-category writeups live in `docs/wpt-vendor-notes/<slug>.md` (2026-08-09 split, see that section's own note for why). |
| `docs/wpt-vendor-notes/` | One file per vendored WPT top-level category (`<slug>.md`, `/` → `-`), holding the full vendoring writeup that `tests/wpt/VENDOR.md`'s and `docs/wpt-status.md`'s category rows used to carry inline — split out 2026-08-09 once both files' row cells had grown past the `Read`-tool size limit. Linked from both tables; create alongside a new short row, don't grow the row itself. |
| `docs/build-speed.md` | Compile-time optimization plan: current baseline, measurement protocol (S1–S5), ranked measures (stable / nightly / rejected), benchmark journal. Read before changing build config (profiles, `.cargo/config.toml`, sccache). |
| `docs/ci-offload.md` | **Plan for moving part of the task gate to GitHub Actions** (the repo is public, so standard runners are free and unlimited): what moves and what stays local, cache/checkout/concurrency strategy under the 10 GB cache cap, how the assistant consumes run results via `gh`, and the rollout order. **Mostly implemented as of 2026-08-20** (six jobs in `ci.yml` including `artifact` (CI-5, dev-release upload on push-to-main), cache/checkout/concurrency, `bench-gate.yml` deleted; §8 realigned with the per-commit merge rule, CI-6; PERF-7 landed as its own `perf-gate.yml`, all three 2026-08-20); the only remaining tail is `snapshot-cpu` as a blocking gate, blocked on [BUG-784](bugs/BUG-784-OPEN.md). Read before touching `.github/workflows/*` or proposing "let CI do it". |
| `docs/lint-policy.md` | **Plan for turning this file's prose policies into machine-checked lints** (no `unwrap`/`panic` in prod, `// SAFETY:` on every `unsafe`, `///` on public items): ready-made clippy rules via `[workspace.lints]`, `clippy.toml` deny-lists, custom `dylint` rules, rollout protocol (one lint per commit, `warn` → fix → `deny`) and journal. Also records why forking rustc and the nightly compile-speed measures were rejected. Read before adding a new "must"/"forbidden" rule here — a rule a lint can check belongs there, not in prose. |
| `docs/probe-method.md` | **How to measure engine behaviour with a probe, and how to read a WPT failure** (what counts as evidence — your own server, not the page and not the browser log; how to launch a probe; why a report naming one defect has 2–7; WPT triage: a wall of TIMEOUTs is one hung page, a partial subtest list names where it stopped; blind spots of source-grepping scripts). Distilled from the WPT-RUN-5/6 slices and the August 2026 bug wave. Read before writing a probe or triaging a run; the per-bug numbers stay in `bugs/BUG-NNN-*.md`. |
| `docs/perf-method.md` | **How to measure, gate and accept a performance change** (census before the fix, profile the path you change, gate by counter/identity not wall-clock, interleaved A/B compared on min, instrumentation traps). Distilled from the 27 slices of BUG-341. Read before a perf task; the numbers themselves stay in `bugs/BUG-NNN-*.md`. |
| `docs/perf/experiment-wgpu-only.md` | **Frozen journal of the deleted `p1-exp-wgpu-only` polygon** (26 perf slices with before/after numbers, the borrowing map, the traps list). **Any code comment under `crates/` citing `EXPERIMENT.md §N` or `EXPERIMENT.md п.N` points here** — the file used to live only on that branch, which was deleted 2026-08-31. Historical reference only: nothing in it is assigned, its PowerShell harness (`scripts/exp/*`) was never ported, and its "OpenGL removed" premise does not hold in `main` (femtovg is kept as the fallback, [ADR-017](docs/decisions/ADR-017-wgpu-default-backend.md)). |
| `docs/automation.md` | **All automation/introspection surfaces of the browser and when to apply them** (dump modes, `--deterministic`, MCP tools/resources, BiDi, IPC, driver-API, `LUMEN_NO_*` paint-bisect flags, known stubs). Read before writing a debugging script or a new test harness — the capability usually already exists. |
| `docs/roadmap-trees.md` | **How to use the interactive roadmap trees** (`docs/roadmap-*.html` — committed viewers; `python scripts/gen_roadmap.py` only substitutes their `<script id="roadmap-data">` block, it cannot create the file): open in a browser, filters/search, and where their data comes from (`ROADMAP.md`, auto-pulling bug status from `BUGS.md`). Regenerating is no longer required in every commit — CI checks that the generator runs, not that the output matches byte for byte. |
| `ROADMAP.md` | Flat, grep-friendly source of the phase/task tree (two markdown tables: phases + tasks, one task per line). Feeds `gen_roadmap.py`; replaced the old nested `docs/roadmap.json`. Bug↔task links live in its `bugs` column; CSS-module status is live-aggregated from `CSS-SPECS.md` into rows `css-specs-t0`…`t4` (note = `AUTO:CSS-SPECS:T<N>`, do not hand-edit that note). |
| `CLAUDE.md` | (this file) Conventions and invariants for the assistant. |
| `REVIEW.md` | **Code-review policy** — what a reviewer blocks a change on (hard gates), architecture boundaries, style/design rules, docs that must move in the same commit, and what is explicitly out of scope for review. Read before reviewing someone's diff; the rules restate this file and `docs/conventions.md` in reviewer form, so on conflict trust the code and flag the change. |
| `docs/decisions/` | Formal ADR files (one per architectural decision). See README.md + TEMPLATE.md inside. |
| `DECISIONS.md` | Historical decisions (pre-ADR format). Read-only — add new decisions to `docs/decisions/` instead. |
| `samples/page.html` | Test page for pipeline runs. |
| `assets/fonts/Inter-Regular.ttf` | Bundled font (SIL OFL 1.1). |
| `assets/fonts/Ahem.ttf` | Bundled font (3-Clause BSD, `assets/fonts/LICENSE-Ahem.txt`) — deterministic reftest font (TEST-5), vendored unmodified from `tests/wpt/fonts/Ahem.ttf`. Every glyph is a 1em solid square; picked up by lumen-font's asset-dir scan like any other bundled family, no dedicated registration code. |

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
| **P2** | **Reactivated 2026-07-13**: leads P2-wpt (WPT via `wptrunner` + WebDriver BiDi, `docs/tasks/p2-wpt-integration.md`) and the DEVX track (dev-tooling on existing automation surfaces, `docs/automation.md`, ROADMAP.md DEVX-1…6, assigned 2026-07-16). Was reserve (since 2026-06-18). **Owns the CI track since 2026-08-19** (`docs/ci-offload.md`, ROADMAP.md `CI-5`/`CI-6`/`PERF-7`), handed over by P5 whose role forbids the behaviour changes the tail needs. | `lumen-bidi-server`, `lumen-driver`/`lumen-mcp` (DEVX-5), Python tooling `tests/wpt/` + `graphic_tests/run.py` (DEVX-1/4), `.github/workflows/*` |
| **P3** | **Bug fixes ONLY**: BUGS.md OPEN items, graphic test regressions. **Skip a row marked `OPEN (ДОРАБОТКА → <task>)`** — that record describes functionality that was never implemented, not a defect in implemented code, and is owned by the named `ROADMAP.md` task instead (2026-08-28; see BUGS.md's own legend for why the file is not renamed) | All crates (read-only except bug fixes) |
| **P4** | **CSS properties ONLY**: parsing, ComputedStyle, cascade, end-to-end wiring | `css-parser`, `layout` (style.rs), `paint` (display_list.rs) |
| **P5** | **Code health ONLY**: audit, workspace-clippy, stub/branch/docs/dep sweeps, safe mechanical cleanup | All crates (read-only except trivial clippy fixes in own crate + branch/worktree cleanup) |

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

**Push after every commit, not at session end.** Each commit is merged into `main` and pushed to `origin/main` as soon as it is made (see "Git workflow" below) — nothing waits for the end of a task or a session, because other parallel sessions rely on seeing it to avoid duplicate or conflicting work.

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
5. **If the property affects paint/rasterization**, regenerate the deterministic CPU snapshot references in the **same commit**: `SAVE_CPU_SNAPSHOTS=1 cargo test -p lumen-driver --features cpu-render cases::snapshot_cpu` (then review the changed PNGs are correct, not garbage). Skipping this drifts `graphic_tests/snapshots/cpu/` on unrelated pages and later red-lights the `scoped-test.sh` gate for someone else — the recurring BUG-118 / BUG-149 / BUG-297 / BUG-316 staleness. **The PNGs are not the only golden set** — a change to the *geometry or order* of emitted commands also drifts the textual display-list snapshots in `crates/engine/paint/tests/snapshots/*.snap` (`UPDATE_SNAPSHOTS=1 cargo test -p lumen-paint --test all <name>`, then read the diff — it is three lines, so a wrong rect is obvious). Those are a plain `cargo test -p lumen-paint` away, i.e. they red-light step 1 of the task checklist for every role that touches paint, not just the graphic gate: BUG-816 left `main` red for a day that way.

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

**The boundary is decision ownership, not subsystem name** ([ADR-027](docs/decisions/ADR-027-own-vs-vendored-boundary.md), 2026-08-31). Ours wherever *we* decide what correct means (layout, cascade, paint order, browsing contexts, what the browser shows or refuses to send); vendored wherever a committee already decided and wrote it into a spec (file format, Unicode table, OpenType lookup, URL state machine, compression, crypto). The test: *if we implement this ourselves and disagree with the reference implementation, are we wrong by definition?* — if yes, take the implementation. This replaced §5's name list, which had drifted out of sync with the code in three places (it claimed an own PNG decoder and an own DEFLATE — both are `zune_png`/`flate2` — and listed `tiny-skia` as never-take while it is a `lumen-paint` dependency). `rustybuzz`, `ttf-parser`, `resvg`/`usvg` and `url` left the never-take list; `html5ever`, `cssparser`, `stylo`, `taffy`, `hyper`, `hickory-resolver`, `encoding_rs`, `adblock`, `readability`, `tokio` and `egui`/`iced`/`Slint` stay forbidden permanently — those are the engine. The trait-anchor and per-dependency justification rules below are unchanged.

### No new dep without justification

Every new `[dependencies]` entry requires this in the commit body:

> **Why this dependency:** \<category (permanent / provisional), trait-anchor, graduation criterion if provisional\>

---

## Code conventions

Full details (style, tests, error handling, unsafe) — [`docs/conventions.md`](docs/conventions.md).

- **Rust pinned to 1.97.0** (`rust-toolchain.toml`, since 2026-08-19 — it was a floating `"stable"`, which handed developers and CI runners different lint sets and made clippy green locally / red in CI). Edition 2024, resolver "3", MSVC on Windows. Raise the pin in its own commit after a clean `RUSTC_WRAPPER= cargo clippy --workspace`: a version bump invalidates every artifact in `target/` (stale ones surface as `E0786 "only metadata stub found for rlib dependency core"`, which reads like a broken dependency but is cured by `cargo clean`) and can add new lints.
- `cargo clippy -p <crate> --all-targets -- -D warnings` must pass before every commit.
- **A new `.rs` file must be ≤2000 lines, and a file already over that must not grow.** Machine-checked since 2026-08-26: `scripts/check_file_sizes.py` (blocking CI job `file-size`) compares every tracked `.rs` against `scripts/file-size-baseline.tsv`. Growth is not forbidden, it is made visible — run `--update` so the number moves in the same commit's diff and explain it in the commit body; what the gate stops is growing unnoticed, which is how `box_tree.rs` gained 919 lines and `network/src/lib.rs` 418 while the SPLIT track's prose rule said neither should. Five table-like files are exempt; rule and rationale — [`docs/lint-policy.md`](docs/lint-policy.md) §5.1, where to put the code instead — [`docs/tasks/p1-monolith-split-queue.md`](docs/tasks/p1-monolith-split-queue.md) §2.
- **sccache is on again and requires ≥ 0.17.0** (`rustc-wrapper` in `.cargo/config.toml`, re-enabled 2026-09-01). Version 0.15.0 crashed every compiler invocation under toolchain 1.97.0 with `STATUS_STACK_BUFFER_OVERRUN` (`0xc0000409`) on crates with a long command line — `rustc` as well as `clippy-driver` — which is why the wrapper was off between 2026-08-19 and 2026-09-01; 0.17.0 passes both scenarios (`cargo test -p lumen-layout`, `cargo clippy -p lumen-driver --all-targets`) and caches. If your `sccache --version` is older, `cargo install sccache --version 0.17.0` before building, or every build dies. Own workspace crates are **not** cached (`incremental = true` makes them non-cacheable) — the win is on dependencies, i.e. a fresh worktree or after `cargo clean`. CI keeps the wrapper off (`RUSTC_WRAPPER: ""`, no sccache on runners).
- **`///` doc comments on all public structs, fields, and functions** — mandatory and **machine-checked** since 2026-08-18 (`missing_docs = "deny"` in `[workspace.lints.rust]`). Pre-existing debt (1866 items) is held behind **file-scoped** `#![allow(missing_docs)]` in 121 files, so a *new* file must document its public API even in a crate that still owes docs; the per-crate counts live in `docs/lint-policy.md` §10.
- No `panic!` / `unwrap()` / `expect()` in production code; allowed in tests. **All three machine-checked** since 2026-08-18 (`clippy::panic`, `clippy::unwrap_used`, `clippy::expect_used`, all `deny` in `[workspace.lints]`). `clippy.toml`'s `allow-*-in-tests` frees only the body of a `#[test]` fn, so test roots and `#[cfg(test)] mod`s carry their own `#![allow]`. Grandfathered sites carry a function-scoped `#[allow]` (an `impl`-scoped one where the function could not be identified), never a crate-scoped one; every allow is listed in `docs/lint-policy.md` §10.
- `unsafe` forbidden outside FFI boundaries; every block requires `// SAFETY:` comment. **Machine-checked** since 2026-08-18: `clippy::undocumented_unsafe_blocks = "deny"` in `[workspace.lints]`, so a missing comment is a build error, not a review finding. One comment above two adjacent `unsafe impl`s does not count as documenting the second.

All three prose policies above are now enforced by `[workspace.lints]` — tier 0 of [`docs/lint-policy.md`](docs/lint-policy.md) was completed on 2026-08-18 (five rules enabled, one rejected on measurement). What remains is the grandfathered debt behind `#[allow]`, listed per crate with owners in that file's §10. Rules are enabled in `[workspace.lints]` (root `Cargo.toml`), configured in `clippy.toml`; every member crate opts in with `[lints] workspace = true`. **A new crate must carry that stanza** or it silently escapes every project lint — `/lumen-new-crate` adds it.
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

**Merge and push after EVERY commit** (user, 2026-08-19). Do not accumulate commits on a branch until the task is done: local gate → commit → `git merge --no-ff` into `main` → `git push origin main`, per commit. Feature branches and `--no-ff` stay mandatory; what changed is the cadence. Rationale: unpushed work does not exist for anyone else — on 2026-08-19 several roles held unmerged branches at once, the root checkout trailed `origin/main` by 21 commits, and parallel sessions duplicated work and collided on bug numbers. Frequent small merges also trade one large conflict for several trivial ones.

**Do not wait for CI before merging** (same decision). The local gate is the only pre-merge check; watch CI on `main` afterwards and fix it if it goes red. Waiting per-commit would cost ~30 min each.

**Task-completion checklist** (all mandatory, full details in `docs/git-workflow.md`). Steps 1–3 already ran per commit under the rule above; at task end they cover whatever the last commit left:
1. `cargo clippy -p <crate> -- -D warnings` + `cargo test -p <crate>`
2. `git merge --no-ff p<N>-task-name -m "Merge …"`
3. `git push origin main`
4. Delete pointer line from `STATUS-PN.md`, commit — then merge and push it too
5. `bash scripts/worktree-pool.sh release p<N>-work` then `git branch -d p<N>-task-name` (a slot holding the branch makes `branch -d` fail)
6. Pool slot — nothing to remove (freed in step 5). Ad-hoc worktree — `git worktree remove .claude/worktrees/<task-name>`
7. `git push origin --delete p<N>-task-name` if the branch was ever pushed — the remote task branch has served its purpose

**When the root checkout blocks the merge.** `main` is checked out in the repo root and often carries someone else's uncommitted files, so `git merge` there fails on paths you touched. Merge in a throwaway worktree instead — `git worktree add .claude/worktrees/merge-tmp -b <tmp> origin/main`, merge, `git push origin HEAD:main`, remove it. Note the local `main` then trails `origin/main`, which also makes `worktree-pool.sh release` refuse to free the slot (it compares against local `main`); free it with `git checkout --detach origin/main` inside the slot.

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
| Bug fixed | `BUGS.md` → `BUGS-FIXED.md` | **move** the row to the archive with status `FIXED <date>` (it is not flipped in place — 548 of 918 rows were closed ones, 2/3 of a file read in every session), rename `bugs/BUG-NNN-OPEN.md` → `-FIXED.md`, then `python scripts/remap_status_pointers.py --apply` — moving a row shifts every `STATUS-PN.md` pointer below it |
| Bug turns out to be a **feature gap**, not a defect | `BUGS.md` + `ROADMAP.md` + `bugs/BUG-NNN-OPEN.md` + `STATUS-P3.md` | status cell → `OPEN (ДОРАБОТКА → <task>)`; add the `ROADMAP.md` task row (bug id in its `bugs` column) and re-run `gen_roadmap.py`; add a `**Тип:**` line under the bug file's status; delete the pointer line from `STATUS-P3.md`. **Do not rename or move the bug file** — CLAUDE.md, STATUS files and the python tooling reference it by path, and the record of observations stays useful where it is |
| CSS property (P4) | `CSS-SPECS.md` + `CAPABILITIES.md` | ⬜ → ✅ |
| New dependency | `docs/plan/tech-stack.md` | append row |
| Architectural decision | `docs/decisions/ADR-NNN.md` | new file from TEMPLATE.md; update index |
| **Live** gotcha found (a trap you can still walk into) | `CLAUDE.md` → "Known gotchas" | append one bullet, 1–3 lines, with the OPEN bug ref |
| Gotcha's defect **fixed** | `CLAUDE.md` → "Known gotchas" | **delete the bullet.** The narrative stays in `git log` + `bugs/BUG-NNN-FIXED.md`; a residual moves into the OPEN bug that tracks it. Do not rewrite the bullet as "~~was~~ — fixed" — that is how this section reached 163 KB |
| Method lesson learned from a bug (how to probe, what counts as evidence) | `docs/probe-method.md` (perf → `docs/perf-method.md`) | append the rule; keep it about the method, not about the defect |
| Perf slice merged with a transferable lesson | `docs/perf-method.md` | append the rule (one paragraph, slice ref in brackets); per-slice numbers stay in `bugs/BUG-NNN-*.md` |
| New public API — or any edit that shifts line numbers under `crates/` | *nothing to commit* | `SYMBOLS.md` is **generated and gitignored** since 2026-08-31. Regenerate it locally when you use it (`python scripts/gen_symbols.py`, ~2 s); it is a derivative of `crates/`, so keeping it in the index only meant every line-shifting commit carried a 723 KB diff that collided with the other roles' merges. |
| Roadmap/bug/CSS-module status change | `ROADMAP.md` | edit `ROADMAP.md` if structure changed; CSS-module and bug status need no edit — `gen_roadmap.py` re-pulls them from `CSS-SPECS.md`/`BUGS.md`. The trees `docs/roadmap-*.html` stay committed (they are hand-written viewers — the generator only fills their data block), but a regeneration no longer has to ride along in every commit: run `python scripts/gen_roadmap.py` (~0.5 s) when you want to look at one. |

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

**Live traps only.** A gotcha describing a defect that has since been *fixed* does not belong here: the narrative is in `git log` and in `bugs/BUG-NNN-FIXED.md`, the residual it left behind belongs in the OPEN bug that tracks it, and the transferable *method* lesson belongs in [`docs/probe-method.md`](docs/probe-method.md) or [`docs/perf-method.md`](docs/perf-method.md). This section is read in full at the start of every session, so a line earns its place only by being something you can still walk into today. (Cut from 163 KB to this on 2026-08-31; the deleted text is in the history of this file.)

### Repo & tooling

- **Cargo.lock is committed** (workspace includes a binary).
- **Line endings:** `.gitattributes` enforces LF. Git warning about CRLF→LF is normal.
- **Archives in repo root are gitignored** (`/*.zip`, `/*.tar*`).
- **`.ignore` (repo root) keeps ripgrep out of `.claude/worktrees/`** — those are full checkouts of other roles' branches, 75 % of all `.rs` files reachable from the root. Without it every search reads the project four times and mixes hits from stale branches into the results. Do not delete it; git does not read this file, so it affects search only.
- **A newline inside any `ROADMAP.md` table cell silently truncates the whole task table.** `gen_roadmap.py::_table_rows` stops at the first line not starting with `|`, so a row split across two lines hides every row below it — no error, no warning, the trees just render fewer tasks. One task = exactly one line. Verify by counting the generated tree (`grep -c '"id":' docs/roadmap-B-twotrees.html` must grow by exactly the rows you added) and grep the html for your own ids. The generator's final line reports *bugs*, not tasks — a number read off it checks nothing.
- **Never round-trip a repo markdown file through Python text mode.** `BUGS.md` contains raw CR bytes inside table cells; universal-newline reading turns each into a row split, silently exploding rows you never opened — and that invalidates every `STATUS-PN.md` pointer below the damage, since those are bare line numbers. Pass `newline=''` on **read** as well as write, or use binary mode / `Edit` / `sed`.
- **Parallel sessions in the same working tree = disaster.** Two sessions checking out different branches makes git stash one session's work, and `git stash pop` recovery is fragile. Hence the mandatory worktrees. If you find yourself on a foreign branch — check `git stash list` before running `git restore .`.
- **An interrupted `git worktree add` leaves an index in which the whole repository is staged as deleted.** The checkout of this repo takes minutes (≈59 000 files, `tests/wpt` dominates), so a tool timeout hits it routinely; the leftover `.git/worktrees/<name>/index.lock` then makes a later `reset --hard` exit 0 without repairing the index. A commit made in that state deletes the entire tree except your own file. **Run `git status --short` in a fresh worktree before the first `git add`, and check `git diff --cached --stat` before every commit made in one.** (Walked into on 2026-08-31; the bogus commit was caught only because an unrelated dirty file in the root blocked the merge.)

### Build & platform

- **Rust is pinned to 1.97.0** and **sccache must be ≥ 0.17.0** (an older one crashes every `rustc`/`clippy-driver` call with `0xc0000409`) — see §Code conventions. A stale `target/` after a toolchain change surfaces as `E0786 "only metadata stub found for rlib dependency core"`, which reads like a broken dependency and is cured by `cargo clean`.
- **`cargo-fuzz`/libFuzzer cannot run on native Windows** — PE/COFF has no equivalent of the `__start___sancov_*` section-boundary symbols libFuzzer needs, so this is structural, not a toolchain version problem. Use WSL (toolchain installed and verified 2026-08-19, recipe in [`fuzz/README.md`](fuzz/README.md)) or `gh workflow run fuzz.yml`.
- **On Linux, `graphic_tests/dump_golden.py` reports 6 of its 12 checks as mismatches no matter what you changed** — the binary path is hardcoded as `target/<profile>/lumen.exe` and the committed references were generated on Windows, so text metrics differ. Prove display-list neutrality there by an A/B of the dumps themselves (capture, `git stash`, rebuild, capture, `diff -rq`), and never `--update` the references from Linux.

### Core (URL)

- **`lumen_core::url::Url::host()` is deliberately NOT backed by the vendored `url` crate** (LIB-6, 2026-09-01) — it stays a raw best-effort string extraction so the address bar's IDN-spoof guard (`address_bar.rs::guard_display_text`) matches exactly what the user typed. Do not "fix" it to call `inner.host()`/`inner.host_str()`; that would let a WHATWG-normalized host diverge from the typed text and reopen the spoof the guard exists to catch. Everything else on `Url` (scheme/path/query/fragment parsing) does go through `url` 2.5.8.

### Driving the browser

- **Any paint/scroll number is meaningless without the wgpu backend it was measured on** — check the `[wgpu] adapter: … (…, Vulkan|Dx12)` line in the run's stderr. Same machine, same adapter: one scroll of `lenta.ru` costs 116 ms/frame on DX12 against 53 ms on Vulkan, which looks exactly like a huge engine regression. The backend is chosen at startup and cached in `<exe_dir>/data/paint/backend_probe.txt`, so two checkouts of the same commit can differ; `WGPU_BACKEND=vulkan|dx12|gl` pins it.
- **`fetch()`/`XMLHttpRequest` do nothing in the headless dump modes and on a `file://` page** — the runtime is handed `fetch_provider = None` and answers `false` without logging, so the failure is indistinguishable from a blocked request. A probe that needs real network must drive a live window (`--mcp-live-port`) and be served over http from the same origin.
- **A `file://` URL passed as the initial CLI page argument does not load** ([BUG-651](bugs/BUG-651-OPEN.md)) — `PageSource::from_arg` never strips the scheme (the sibling used by JS/BiDi navigation does). Start on `about:blank` and navigate.
- **Portable user data dir (`<exe_dir>/data/`).** Provisional convention (user decision 2026-06-16): keep portable data in the browser folder via `browser_data_dir()`, never in OS dirs (`%APPDATA%`/`~/.config`) or `lumen_cache_dir()`.
- **`gdigrab` captures the whole desktop, not just the Lumen window** — any OS focus change during a `graphic_tests/run.py` run silently corrupts every screenshot from that point on, and fails as a wall of unrelated-looking high-diff FAILs rather than an error (typing into this chat mid-run did it on 2026-07-28: 72 of 149 tests "failed" at 85–93 % diff, the screenshots being pictures of the chat window). Re-run clean before diagnosing many simultaneous large-diff failures as an engine regression. Backgrounding the run fails loudly instead, at TEST-00.
- **`--screenshot` and the live window (`--mcp-live-port`, used by `graphic_tests/run.py`) can render the SAME page differently** — `--screenshot` goes through `cpu_raster.rs` (the deterministic CPU path), the live window through `renderer.rs` (wgpu) — they are independent implementations of every `DisplayCommand`, not two callers of one renderer. `PushMaskLayer`/`PopMaskLayer` is a concrete case: correct on `--screenshot`, no visible effect in the live window ([BUG-936](bugs/BUG-936-OPEN.md)). A `--screenshot` match against Edge does not prove the live/GPU path is correct — check both before trusting a paint change.

### WPT harness

- **A top-level `/resources/testdriver.js` (repo root, sibling of `tests/`) is not clutter** — `tools/wptrunner/.../environment.py::get_routes` hardcodes that path relative to its own location, which in this repo's vendoring depth lands on the Lumen repo root. Not overridable, and `tools/wptrunner` is unmodified vendor, so this is the only place the file can live. See `tests/wpt/VENDOR.md`.
- **Any `tests/wpt/run_report.py` run can die before the first test with `OSError: Servers failed to start: wss:18889` / `module 'ssl' has no attribute 'wrap_socket'`** — `wptserve` starts every configured scheme unconditionally, and `pywebsocket3` 4.0.2 calls an API removed in Python 3.12. The patch lives only in `tests/wpt/.venv/…/websocket_server.py`, which is not committed (`.venv/.gitignore` is a bare `*`), so **re-apply it on every fresh venv and in every pool slot**.
- **A live WPT run does not see your edit to `testharnessreport.js`/`testdriver.js`/`testharness_runner.html`** — those routes are `StaticHandler`s that cache file contents for the server process's lifetime, and the report template's `%(…)s` values are substituted from the options of the run that *built* the handler. Since ports are pinned, a server orphaned by a dead run keeps serving the next one (so e.g. a `--timeout-multiplier 3` silently applies to the following run). `tests/wpt/route_audit.py` diffs the served bytes against the checkout; `tests/wpt/port_guard.py --reclaim` kills the squatter. Test *files* are unaffected — those are read per request.
- **A `<meta name=timeout content=long>` test now really gets 60 s** (since [BUG-796](bugs/BUG-796-FIXED.md), 2026-08-24), so a corpus run is *more* expensive than before, ≈ +42.8 h over the full corpus. Tell the two timeout sources apart in the records: `test_status` carrying the harness's own `"Test timed out"` at ~10 s means the page decided; `test_timeout: 60` in `test_end`'s `extra` with no `test_status` means wptrunner cut it. **Every ~10 s TIMEOUT recorded on a `long`-declared test before 2026-08-24 says nothing about its subject** — the whole WPT-RUN-5/6 snapshot needs re-measuring.

### Engine invariants (JS / shim)

- **A per-feature shim outside `WEB_API_SHIM*` is its own `rt.eval` that a page-shim fix never reaches.** `xhr.rs`, `audio_element.rs`, `video_bindings.rs`, `web_audio.rs`, `worker.rs`, `broadcast_channel.rs`, … each install their own JS. The same defect has been fixed three times in different modules for exactly this reason (relative-URL resolution in `fetch`, then XHR, then `sendBeacon`). Before assuming a fix landed everywhere, grep the other shims for the same shape.
- **`_LUMEN_WRAPPER_MEMBERS` (`crates/js/src/dom.rs`) sits on a prototype BELOW the interface prototypes**, so anything declared there outranks every `_lumen_install_reflection` row and every hand-written `HTML*Element.prototype` accessor. A tag-specific member in that shared table silently swallows the same-named IDL attribute of every other interface — that is how `meta.content` answered `undefined` while `getAttribute('content')` worked. Nothing tag-specific belongs in it. The same shadowing shape is live on `iframe.src` ([BUG-920](bugs/BUG-920-OPEN.md)).
- **A `thread_local!` set inside an `install_*_v8` function and read inside one of the natives it registers reads back its DEFAULT** — the installer and the native's invocation run on different OS threads (measured: `ThreadId(2)` vs `ThreadId(3)`). Compute the value once before `rt.register_native` and capture it **by value** in a `move` closure, the way `offscreen_canvas.rs` does; never round-trip install-time state through a thread-local.
- **A JSON payload crossing the engine↔shim boundary must be encoded the way the receiver reads it.** `fire_popstate` embedded a JSON object bare into a call whose shim side ran `JSON.parse` on it, so *every* traversal delivered `state: null` — and it went unnoticed for months because `null` is the one value that round-trips through that confusion unchanged. A working `null` proves nothing about any other value.
- **Queue a callback the shim makes on the page's behalf as a task, do not dispatch it inline.** WPT arms its `EventWatcher` *after* the triggering call, so a synchronous event is actively worse than none (measured −2 subtests synchronously against +1 as a task). The deliberate exception is `statechange` in `web_audio.rs`: this engine pumps timers only when it redraws, so on a static page a task waits up to a second.
- **Anything added to a JS prototype must be non-enumerable** — `web-animations`' `style-change-events.html` builds one subtest per `Object.keys(Animation.prototype)` entry, so an enumerable helper invents failing subtests named after engine internals.
- **`Lumen::js_ctx` is `None` in a live window.** Since ADR-023 the engine thread is on by default and `set_js_ctx` deposits the handle in *its* state, so code reading `self.js_ctx` directly silently does nothing. Go through `route_task_js`/`route_query_js`, or `Lumen::clone_js_ctx` when a router's single `FnOnce` will not do. A sub-document's runtime is reachable by neither — call `eval_js`/`eval_js_value` on the frame handle.
- **`lock_document_bounded` is the only bounded-wait lock in `crates/js`**, and it exists because a window `load` handler runs *concurrently* with the UI thread's own pass over the document (measured: 3.9 ms of contention). Any new `try_lock` on a JS-visible path inherits that race — a name lookup that declines instead of waiting turns into a `ReferenceError` in `load` handlers and nowhere else.
- **Anything that changes the stylesheet set from Rust must move `inline_style_fingerprint` or `stylesheet_link_fingerprint`** (`doc_extract.rs`) — since [BUG-443](bugs/BUG-443-FIXED.md) the cascade is built *before* the document's scripts and rebuilt only on a fingerprint mismatch, so a new path that adds a `<style>`/`<link>` without touching either is invisible to the rebuild.
- **`--screenshot`, `--dump-display-list` and the live window each write the page's display list by their own path**, and frame content is spliced in on *every* write. A fourth path added without the splice silently shows the grey `<iframe>` placeholder (that is how `--screenshot` shipped without it). The pixel goldens do not cover frames at all: `lumen-driver` builds its own display list and `run.py`'s pages contain no `<iframe>`.

### Live engine gaps a probe will walk into

These are open defects, not history — a probe that depends on one of them measures the bug instead of its subject. Full list: `BUGS.md`.

- **`<img>` fires neither `load` nor `error` on any insertion path, and `img.complete` is `undefined`** ([BUG-630](bugs/BUG-630-OPEN.md)). Never sequence a probe on an image arriving, and never read a silent `<img>` as evidence that a policy blocked it.
- **An event dispatched from script reaches only the node it was dispatched on** — no ancestor, no `document`, no `window`, in either phase; a native click reaches `document` but not `window`; `event.target` is unset and `eventPhase` is `undefined` ([BUG-873](bugs/BUG-873-OPEN.md)). Also: `document.on<type> = fn` sticks as a property and is never invoked, and `'onX' in Y` answers `false` although assignment works ([BUG-874](bugs/BUG-874-OPEN.md)). Listen with `addEventListener` on the target itself; never feature-detect with `'onX' in Y`.
- **`window.open()` and `<a target=_blank>` replace the *calling* document** ([BUG-883](bugs/BUG-883-OPEN.md)) — the opener's timers never fire again. A frame inserted after the shell's single sub-document pass (from a `load` handler, a timer, rAF), or a `src` assigned to an already-inserted frame, produces no request at all ([BUG-885](bugs/BUG-885-OPEN.md)); a frame built by a top-level inline script loads fine. Write frames into the markup with their final URL.
- **`window.postMessage` accepts only the legacy string `targetOrigin`** — `'*'`, the exact origin and `'/'` work; both dictionary forms, the one-argument form and a trailing slash drop the message silently ([BUG-717](bugs/BUG-717-OPEN.md)). The cheapest way to sequence a probe page therefore needs the literal `'*'`.
- **A spacer that paints nothing gives the page no scroll** — `content_height` comes from the display list, so `<div style="height:4000px">` leaves `max_scroll()` at 0 and `scrollTo` genuinely does nothing. Give it a background.
- **CSP is parsed and never enforced**, and `securitypolicyviolation` is dispatched nowhere ([BUG-811](bugs/BUG-811-OPEN.md)) — a wait on it can only hang.
- **The CSSOM is absent**: `document.styleSheets` and `<style>/<link>.sheet` are `undefined`, no rule class is a global, `new CSSStyleSheet()` throws ([BUG-471](bugs/BUG-471-OPEN.md)); `adoptedStyleSheets` is a plain expando. Also missing as globals: `DOMRect`/`DOMPoint`/`DOMMatrix`, `StaticRange`, `XSLTProcessor`/`document.evaluate`, `document.forms`/`scripts`/`links`, `getClientRects` ([BUG-478](bugs/BUG-478-OPEN.md)), `window.innerHeight`/`innerWidth` ([BUG-529](bugs/BUG-529-OPEN.md)), `document.defaultView` ([BUG-622](bugs/BUG-622-OPEN.md)).
- **`test_driver.click(element)` cannot work**: `testdriver.js` opens with `element.getClientRects()`, which does not exist, so it throws synchronously — fixing `defaultView` alone unblocks nothing. Of ~30 `test_driver_internal` actions the executor implements two (`click`, `generate_test_report`) ([BUG-810](bugs/BUG-810-OPEN.md)).
- **`PerformanceObserver.supportedEntryTypes` lists `layout-shift` and never delivers one** ([BUG-809](bugs/BUG-809-OPEN.md)) — the advertisement is why such tests TIMEOUT instead of failing. The list is a promise about the *type*, not about delivery: check for a call site before believing it.
- **`new WebSocket(url)` blocks the whole document until the handshake settles** ([BUG-856](bugs/BUG-856-OPEN.md)), and `send()` throws on anything that is neither string nor buffer ([BUG-862](bugs/BUG-862-OPEN.md)). Open one only against a server you control.
- **No outgoing request carries `Referer` or `Origin`** — not a subresource, not `fetch()`, not a same-origin POST ([BUG-859](bugs/BUG-859-OPEN.md)), although `docs/plan/privacy.md` promises `strict-origin-when-cross-origin`.
- **`<object data>` and `<embed src>` never fetch** ([BUG-798](bugs/BUG-798-OPEN.md)); `<input type=image>` and SVG `<image>` fetch but fire no `load`/`error`. A probe needing a subresource should use `<link rel=stylesheet>`, `<script src>` or `fetch()`.
- **An `.xhtml`/`.xht`/`.svg` page runs no scripts** — navigation has no XML path at all, so the file is HTML-parsed ([BUG-786](bugs/BUG-786-OPEN.md)): a prefixed `<h:script src>` is never requested, a self-closing `<script src="…"/>` swallows the rest of the document, and `<![CDATA[` is a syntax error. Never CDATA-wrap a probe's script.
- **Shadow trees do not slot**: no slottable is ever assigned, `slotchange` fires nowhere ([BUG-876](bugs/BUG-876-OPEN.md)), `host.shadowRoot` returns a fresh wrapper per read ([BUG-877](bugs/BUG-877-OPEN.md)), and a `<script src>` in a shadow root is never requested ([BUG-878](bugs/BUG-878-OPEN.md)).
- **`javascript:` URLs never execute** anywhere ([BUG-884](bugs/BUG-884-OPEN.md)); `window.close()` is a no-op and `window.closed`/`name` are `undefined` ([BUG-887](bugs/BUG-887-OPEN.md)); `document.open()`/`close()` do not exist ([BUG-888](bugs/BUG-888-OPEN.md)); an entry made by `history.pushState(state, "")` (no URL argument) fires no `popstate` on traversal ([BUG-886](bugs/BUG-886-OPEN.md)).
- **An `<audio>` `src` is not resolved against the document base**, so a relative URL dies as `MEDIA_ERR_SRC_NOT_SUPPORTED` with no request on the server ([BUG-924](bugs/BUG-924-OPEN.md)). `<audio>` and `<video>` are two different models — `<audio>` still dispatches synchronously — so run a media probe against both.
- **A leaked IndexedDB connection stalls every later upgrade and delete on that name** — correct per spec, but it means a probe must close its connections or the next test waits forever.
- **`sessionStorage` has no quota**, so a `while (true)` filling it hangs the page ([BUG-870](bugs/BUG-870-OPEN.md)).
- **`Element.matches(':focus'/':focus-visible'/':focus-within')` answers `false`** even when the `:focus` *style* has been applied — the selector-matching path does not resolve dynamic pseudo-classes ([BUG-560](bugs/BUG-560-OPEN.md)).

When you discover a non-obvious implementation detail in a specific subsystem, add it to [`subsystems/<crate>.md`](subsystems/) under the relevant crate section (in English), not here.

## When in doubt

- **Architecture / scope** — `docs/plan/architecture.md` (§1 Principles, §3 Architecture).
- **Dependency policy** — `docs/plan/tech-stack.md` (§5).
- **How to build / run** — `README.md`.
- **Current code state** — `git log --oneline`.
- **Why a decision was made** — `docs/decisions/ADR-*.md` or `DECISIONS.md` (historical).

If the question isn't answered by these sources — ask the user, don't assume.
