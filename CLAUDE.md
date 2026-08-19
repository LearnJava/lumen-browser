# CLAUDE.md

Project context for Claude Code. Auto-loaded each session. Keeps the assistant oriented without re-asking questions answerable from code or adjacent docs.

**This file is English-only.** All edits — including gotchas added by other sessions — must be written in English. Translate before committing.

Update this file whenever you change architecture, invariants, or policies.

---

## What is this

**Lumen** — private, lightweight, transparent browser in Rust with a custom engine. Not a Chromium/WebKit wrapper; a standalone rendering engine with an embedded JS engine.

Current phase: **Phase 2 — v0.5 «Interactive» (complete)**, app version **v0.5.0**. Phase 0 (prototype) closed 2026-05-26; Phase 1 «Reader» largely complete. Phase 2 delivered: QuickJS, Canvas 2D, CSS Grid, Shadow DOM, accessibility tree, forms, find-in-page, DevTools/CDP, knowledge layer.

**JS engine: V8 (`rusty_v8`) is the ONLY JS engine, full stop.** S12b-F1 (2026-08-04) removed the `quickjs` shell rollback feature; `QuickJsRuntime` itself was deleted in S12b-F2 (2026-08-04); `dom.rs::install_primitives` (the rquickjs native-registration entry point, 2736 lines) was deleted in S12b-F3 (2026-08-04); S12b-F4 (2026-08-04, closing the `P3-v8-s12b` track) removed the `rquickjs`/`rquickjs-core`/`rquickjs-sys` dependency from `crates/js/Cargo.toml` and `Cargo.lock` outright — `rquickjs` is gone from the workspace entirely, not just unused. Never target new functionality, fixes, or investigation at the rquickjs path (it no longer exists); the JS shim (`WEB_API_SHIM` in `crates/js/src/dom.rs`, `#[cfg(feature = "v8-backend")]`) is evaluated only by the V8 install path (`v8_runtime.rs::install_dom`) and is the right place for engine-independent fixes. Validate JS work against the default (V8) build.

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
| `docs/wpt-status.md` | WPT readiness: all 277 upstream top-level categories (scope ⬜/🚫, vendored status), plus a per-test detail table for the one vendored category (`dom/nodes`, 168 tests) with pass/fail and an assignable Владелец/Баг column. Regenerate the detail table with `tests/wpt/gen_status_md.py` after a fresh `run_report.py --all` run — read the file's own "Как обновить" section, not this line, for the exact commands. The category index's "Заметка" cell must stay a one-sentence summary + link — full per-category writeups live in `docs/wpt-vendor-notes/<slug>.md` (2026-08-09 split, see that section's own note for why). |
| `docs/wpt-vendor-notes/` | One file per vendored WPT top-level category (`<slug>.md`, `/` → `-`), holding the full vendoring writeup that `tests/wpt/VENDOR.md`'s and `docs/wpt-status.md`'s category rows used to carry inline — split out 2026-08-09 once both files' row cells had grown past the `Read`-tool size limit. Linked from both tables; create alongside a new short row, don't grow the row itself. |
| `docs/build-speed.md` | Compile-time optimization plan: current baseline, measurement protocol (S1–S5), ranked measures (stable / nightly / rejected), benchmark journal. Read before changing build config (profiles, `.cargo/config.toml`, sccache). |
| `docs/ci-offload.md` | **Plan for moving part of the task gate to GitHub Actions** (the repo is public, so standard runners are free and unlimited): what moves and what stays local, cache/checkout/concurrency strategy under the 10 GB cache cap, how the assistant consumes run results via `gh`, and the rollout order. **Mostly implemented as of 2026-08-20** (six jobs in `ci.yml` including `artifact` (CI-5, dev-release upload on push-to-main), cache/checkout/concurrency, `bench-gate.yml` deleted; §8 realigned with the per-commit merge rule, CI-6, 2026-08-20); the tail — `snapshot-cpu` as a blocking gate (blocked on [BUG-784](bugs/BUG-784-OPEN.md)) — is owned by P2 (`ROADMAP.md` row `PERF-7`). Read before touching `.github/workflows/*` or proposing "let CI do it". |
| `docs/lint-policy.md` | **Plan for turning this file's prose policies into machine-checked lints** (no `unwrap`/`panic` in prod, `// SAFETY:` on every `unsafe`, `///` on public items): ready-made clippy rules via `[workspace.lints]`, `clippy.toml` deny-lists, custom `dylint` rules, rollout protocol (one lint per commit, `warn` → fix → `deny`) and journal. Also records why forking rustc and the nightly compile-speed measures were rejected. Read before adding a new "must"/"forbidden" rule here — a rule a lint can check belongs there, not in prose. |
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

- **Rust pinned to 1.97.0** (`rust-toolchain.toml`, since 2026-08-19 — it was a floating `"stable"`, which handed developers and CI runners different lint sets and made clippy green locally / red in CI). Edition 2024, resolver "3", MSVC on Windows. Raise the pin in its own commit after a clean `RUSTC_WRAPPER= cargo clippy --workspace`: a version bump invalidates every artifact in `target/` (stale ones surface as `E0786 "only metadata stub found for rlib dependency core"`, which reads like a broken dependency but is cured by `cargo clean`) and can add new lints.
- `cargo clippy -p <crate> --all-targets -- -D warnings` must pass before every commit.
- **sccache is off** (`rustc-wrapper` commented out in `.cargo/config.toml`, 2026-08-19): version 0.15.0 crashes every compiler invocation under toolchain 1.97.0 with `STATUS_STACK_BUFFER_OVERRUN` (`0xc0000409`) on crates with a long command line — `rustc` as well as `clippy-driver`, so it breaks plain `cargo test`, not just linting. Re-enable only after sccache is updated and `cargo test -p lumen-layout` passes through it. CI never used the wrapper.
- **`///` doc comments on all public structs, fields, and functions** — mandatory and **machine-checked** since 2026-08-18 (`missing_docs = "deny"` in `[workspace.lints.rust]`). Pre-existing debt (1866 items) is held behind **file-scoped** `#![allow(missing_docs)]` in 119 files, so a *new* file must document its public API even in a crate that still owes docs; the per-crate counts live in `docs/lint-policy.md` §10.
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
| Bug fixed | `BUGS.md` | `OPEN` → `FIXED <date>` |
| CSS property (P4) | `CSS-SPECS.md` + `CAPABILITIES.md` | ⬜ → ✅ |
| New dependency | `docs/plan/tech-stack.md` | append row |
| Architectural decision | `docs/decisions/ADR-NNN.md` | new file from TEMPLATE.md; update index |
| Known gotcha found/fixed | `CLAUDE.md` → "Known gotchas" | append/remove bullet |
| Perf slice merged with a transferable lesson | `docs/perf-method.md` | append the rule (one paragraph, slice ref in brackets); per-slice numbers stay in `bugs/BUG-NNN-*.md` |
| New public API — **or any edit that shifts line numbers under `crates/`** | `SYMBOLS.md` | `python scripts/gen_symbols.py`. The index stores `file:line`, so inserting a comment or an attribute drifts it just as much as adding an item. Since 2026-08-19 the CI `doc-drift` job is **blocking** and fails on the diff, so this is no longer optional bookkeeping. |
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
- **The committed `.cargo/config.toml` sets `build.rustc-wrapper = "sccache"`, so *any* environment without `sccache` on PATH fails instantly on every cargo command** — `error: could not execute process 'sccache …/rustc -vV' (never executed)`, before a single crate is compiled. This kept GitHub Actions red for two months: the config landed 2026-06-17 (`2ffbee511`), and `ci.yml` failed on all three OS at `cargo check -p lumen-shell` in every one of 1187 runs on `main` — nobody looked, because nothing surfaced the failure locally. Found 2026-08-18 (P5) while auditing CI for `docs/ci-offload.md`. Fixed by `RUSTC_WRAPPER: ""` in the `env:` of `ci.yml`/`release.yml`/`bench-gate.yml` (the last of these was itself deleted on 2026-08-19, `docs/ci-offload.md` §11.5 — don't go looking for it; an empty value overrides the config and disables the wrapper — verified A/B against `sccache --show-stats`, remembering that the counter lives on the server at `SCCACHE_SERVER_PORT=4444` set by that same config, not on the default port). **Consequence for any new automation** — a fresh container, a second machine, a colleague's checkout: install sccache or export an empty `RUSTC_WRAPPER`, otherwise cargo dies with a message that names sccache but reads like a toolchain failure.
- **A newline inside any `ROADMAP.md` table cell silently truncates the whole task table.** `gen_roadmap.py::_table_rows` collects rows "until the first line not starting with `|`", so a row split across two lines makes every row below it invisible to the generator — no error, no warning, the trees just render fewer tasks. Found 2026-08-19: the `WPT-RUN-3` row had been broken in two, hiding 19 tasks (the generator's own summary went 247 → 273 once repaired), and rows appended later landed in the dead zone. One task = exactly one line; after editing `ROADMAP.md` check the generator's final "всего задач: N" line, not just that it exited cleanly.
- **Line endings:** `.gitattributes` enforces LF. Git warning about CRLF→LF is normal.
- **Archives in repo root are gitignored** (`/*.zip`, `/*.tar*`). Downloaded files won't accidentally get committed.
- **Portable user data dir (`<exe_dir>/data/`).** The ad-block external-filter-list subsystem stores its data under `<exe_dir>/data/adblock/` (SQLite `adblock.db` for subscriptions + list metadata; `lists/<slug>.txt` bodies; `custom-rules.txt`) — see `shell/src/adblock.rs::browser_data_dir`. This is a **provisional** convention (user decision 2026-06-16): keep everything in the browser folder, do **not** use OS dirs (`%APPDATA%`/`~/.config`/`~/.cache`) or `lumen_cache_dir()`/`config_path()` for portable data. New subsystems needing portable data should add their own `data/<subsystem>/` subfolder via `browser_data_dir()`.
- **`gdigrab` captures the whole desktop, not just the Lumen window — any OS focus change during a `graphic_tests/run.py` run (live-window default, SDC-4) silently corrupts every screenshot from that point on.** Found 2026-07-28 while validating SDC-4: a live-window full run showed 72/149 tests FAIL at 85–93% diff, all starting from one point onward; the "Lumen" screenshots turned out to be pictures of the Claude Code chat window itself — the OS foreground had shifted there (a chat message arrived mid-run) and `_bring_pid_to_front`'s alt-key trick never reclaimed it for the rest of the run. Not a regression from SDC-4 — the same `gdigrab`+bring-to-front mechanism was already used by the old per-test-process default, so the risk is inherent to the whole pipeline, not new. Unlike the backgrounded case (which fails loudly at TEST-00, "magenta marker not found"), a mid-run focus steal fails silently as a wall of unrelated-looking high-diff FAILs — don't diagnose those as engine regressions before checking whether anything touched the desktop (including typing into this chat) while the run was in flight. Re-run clean (no desktop interaction) before trusting a graphic-tests result with many simultaneous large-diff failures.
- **A top-level `/resources/testdriver.js` (repo root, sibling of `tests/`/`crates/`, not under `tests/wpt/`) is not clutter — do not delete or relocate it.** `tools/wptrunner/wptrunner/environment.py::get_routes` hardcodes the `/resources/testdriver.js` static route as `[<repo_root>/resources/testdriver.js, executors/message-queue.js, testdriver-extra.js]` concatenated, where `repo_root` is computed by walking three `os.pardir`s up from `environment.py`'s own path. In upstream wpt that lands on the wpt checkout root (where `resources/testdriver.js` really lives); in this repo, where `tools/wptrunner` is vendored under `tools/` instead of at the repo root, the same walk lands on the *Lumen* repo root instead — an accident of vendoring depth, not a deliberate layout choice. Not overridable via `env_options()` the way `testharnessreport` is (no such hook on this route) and `tools/wptrunner` is not patched (unmodified-vendor rule), so this is the only place the file can live for `test_driver.*` support (WPT-RUN-2) to work at all — see `tests/wpt/VENDOR.md`.
- **Parallel sessions in the same working tree = disaster.** Two sessions doing `git checkout` of different branches causes git to stash one session's work. Recovery via `git stash pop` is fragile. **Solution: mandatory `git worktree`s** (see Worktree isolation above). If you find yourself on a foreign branch — check `git stash list` before running `git restore .`.
- **~~Live-window BiDi/MCP `script.evaluate` can hang indefinitely~~ — misdiagnosis, root-caused 2026-07-17 (P2-wpt S4).** The "`script.evaluate` reports `JS context not available` forever" symptom previously attributed to an environment-dependent JS-runtime-install race was actually three separate, fully deterministic engine/shell bugs, none of them environment-flaky: [BUG-298](bugs/BUG-298-FIXED.md) (`Element`/`DocumentFragment`/`ShadowRoot`.querySelector(All) searched the whole document instead of the calling node's subtree, so any code building a detached DOM subtree and querying into it — e.g. `testharness.js`'s own results-table renderer — silently got nothing), [BUG-299](bugs/BUG-299-FIXED.md) (`Element.prototype.insertAdjacentText` was missing entirely, thrown from the same code path), and [BUG-300](bugs/BUG-300-FIXED.md) (`browsingContext.navigate`'s `DocumentReady` wait could ACK using the *previous* page's stale `layout_box` before the new page had even started loading). All three are now fixed. The `run_smoke.py`-only follow-up timeout was [BUG-301](bugs/BUG-301-FIXED.md), **now fixed too (2026-07-18)**: `wptrunner` registers a static route for `/resources/testharnessreport.js` serving its *own* `__wptrunner_message_queue` report, which wins over the on-disk file — so Lumen's vendored report (the one that sets `window.__lumen_wpt_results`, the global `LumenTestharnessExecutor` polls) was never served under `wptrunner`+`wptserve`, hence "works manually over a plain HTTP server, times out under wptrunner". Fixed by `browsers/lumen.py::env_options` pointing `testharnessreport` at Lumen's own report file. `--bidi-port`/`--mcp-live-port` session-restore racing (a separate, unrelated mechanism) was already fixed by [BUG-296](bugs/BUG-296-FIXED.md). Gotcha when re-checking WPT: a **stale `target/dev-release/lumen.exe`** will mimic an unrelated empty-subresource-fetch bug — rebuild before trusting a `run_smoke.py` failure.

- **A navigation that fails to load reports *success* and silently keeps the previous document** ([BUG-438](bugs/BUG-438-OPEN.md)). `browsingContext.navigate` (BiDi) and the MCP/driver `navigate` both answer `{navigation, url}` / `success: true` for an unreachable URL exactly as they do for a real one, and `location.href` keeps pointing at the old page — there is no error page and no BiDi error. `bc_navigate` (`crates/bidi-server/src/protocol.rs`) only surfaces an error when `LiveWindowSession::navigate` or the `DocumentReady` wait itself fails; a load that never completes is reported asynchronously and never reaches the reply. `LiveWindowSession::navigate` (`crates/driver/src/live_session.rs`) additionally writes the *requested* URL into `current_url` before attempting the load, so `current_url` lies too. Originally filed as a `data:`-only defect; a 2026-08-10 probe (during [BUG-380](bugs/BUG-380-FIXED.md)) widened it to any failing load — closed port, missing file, malformed URL. **Consequence for any script or harness driving the browser:** never treat a successful `navigate` as "the page loaded". Assert on document identity, not on the navigate result and not on a URL comparison (a server redirect breaks that) — mark the outgoing document with a global and check the marker is gone, the way `executorlumen.py`'s `RESET_EXPRESSION`/`STALE_GLOBAL` does.
- **Any paint/scroll performance number is meaningless without the wgpu backend it was measured on — check the `[wgpu] adapter: … (…, Vulkan|Dx12)` line in the run's stderr before comparing runs.** On the same machine and the same adapter (Intel Iris Plus), one scroll run of `lenta.ru` costs 116 ms of frame time on DX12 against 53 ms on Vulkan: `drop(pass)` 0.57 vs 0.20 ms, `prep` 31.4 vs 6.1, `submit` 20.0 vs 8.3, while our own CPU phase (`collect`) is identical — so a backend switch between two measurements looks exactly like a huge engine regression or win ([BUG-405](bugs/BUG-405-OPEN.md) slice 14). The backend is not fixed by the build: it is chosen at startup by `backend_probe::pick_backend` and cached in `<exe_dir>/data/paint/backend_probe.txt`, so two checkouts of the same commit can run different backends. `WGPU_BACKEND=vulkan|dx12|gl` pins it for a measurement (it also disables the probe). Before slice 14 the cache could pin a machine to a backend permanently — a cached winner was probed first, passed, and candidates ahead of it in the Vulkan-first order were never probed again.
- **`fetch()` / `XMLHttpRequest` do nothing in the headless dump modes and on a `file://` page — the page's scripts run, but every request fails as a bare `TypeError: fetch: network error` with nothing on stderr.** The JS runtime installed by `--dump-layout` / `--dump-display-list` is handed `fetch_provider = None` (`crates/shell/src/main.rs`, the `#[cfg(feature = "v8")]` install block), and `_lumen_fetch_sync` answers `false` without logging when the provider is absent, so the failure is indistinguishable from a blocked or refused request. Found 2026-08-17 while validating [BUG-749](bugs/BUG-749-FIXED.md): a probe page loaded from `file://` in a live MCP window reported five network errors against a server that `curl` was reaching at that very moment. **Consequence for any probe that needs real network from page JS:** drive a live window (`--mcp-live-port`) *and* serve the probe page over http from the same origin as the requests — do not load it as `file://`, and do not read a `TypeError` from such a page as evidence about the network stack.
- **A `file://` URL passed as the initial CLI page argument does not load** ([BUG-651](bugs/BUG-651-OPEN.md)) — `--dump-layout`/`--dump-display-list`/`--screenshot`/`--print-to-pdf`, and the initial `<src>` of `--mcp-live-port N <src>`/`--bidi-port N <src>`, all resolve their source via `PageSource::from_arg`, which never strips the `file://` scheme (unlike the sibling `page_source_for_automation_url` used by JS-navigation and BiDi/MCP `navigate` calls, which does this correctly, drive-letter gotcha included) — the whole `file://...` string reaches `PathBuf`/`File::open` and fails (`os error 123` on Windows). Every script in this repo already works around it by starting with `about:blank` then calling the `navigate` MCP/BiDi tool with the real `file://` URL — do the same in any new probe script; do not pass a `file://` URL as the initial CLI/`--mcp-live-port` argument.
- **Any `tests/wpt/run_report.py` run can fail before executing a single test with `OSError: Servers failed to start: wss:18889` / `module 'ssl' has no attribute 'wrap_socket'`, even for a category that itself needs nothing but plain `http`.** `WPT-VENDOR-websockets` (2026-08-18) permanently enabled the `ws`/`wss`/`h2` ports in `tests/wpt/config.json` (previously `null`, which made `tools/serve/serve.py::start_servers` skip those schemes); `wptserve` starts **every configured scheme unconditionally**, so a `wss` startup failure now aborts the whole run regardless of category. The root cause is `pywebsocket3` 4.0.2's `websocket_server.py:160` calling `ssl.wrap_socket()`, removed in Python 3.12+. The fix (replace it with `ssl.SSLContext(PROTOCOL_TLS_SERVER)` + `load_cert_chain` + `wrap_socket(sock, server_side=True)`) lives only in `tests/wpt/.venv/Lib/site-packages/pywebsocket3/websocket_server.py` — **not committed** (`.venv/.gitignore` is a bare `*`) — so it does not survive a venv reinstall or a different pool slot's venv; `WPT-VENDOR-webstorage` (same day) hit it again on a fresh `.venv` despite `websockets` already having "fixed" it locally. Re-apply the same patch whenever this error appears; a durable fix would pin an older `pywebsocket3`/Python in `tests/wpt/requirements.txt` or carry the patch outside `.venv`, neither done yet.

- **`cargo-fuzz`/libFuzzer cannot actually run on this dev machine yet — only builds/links up to the point that needs a runtime it doesn't have.** Found 2026-08-18 building TEST-1 (`fuzz/`, see `docs/tasks/p2-test-track.md#test-1-состояние`). Native `x86_64-pc-windows-msvc` nightly: rustup doesn't ship `librustc-nightly_rt.asan.a`, so `cargo fuzz build` fails at the final link step even though all target crates compile cleanly; dropping to `-s none` (SanitizerCoverage without ASan) instead fails on unresolved `__start___sancov_*`/`__stop___sancov_*` — those boundary symbols are an ELF/Mach-O section-aggregation trick that PE/COFF (Windows' object format) doesn't support the same way, so this isn't a toolchain-version bug, it's a structural gap in Windows fuzzing support (matches upstream `cargo-fuzz`'s own "basic"/experimental framing of `--no-include-main-msvc`). WSL (Ubuntu, the default distro here) has `rustup`+nightly+`cargo-fuzz` installable fine, but has no C toolchain and `sudo apt install build-essential` needs a password nobody scripted has. **Consequence:** any task needing to actually execute a libFuzzer/sanitizer-coverage binary needs either a human running `sudo apt install -y build-essential clang` in WSL once, or a genuine Linux box/CI container — don't spend time on further native-Windows or portable-MinGW workarounds without a concrete reason to expect the PE/COFF section-symbol gap doesn't apply (GNU `ld` targeting the same `-pc-windows-gnu` triple still emits PE/COFF, so it likely doesn't). **Settled on 2026-08-19 by taking the CI route:** `.github/workflows/fuzz.yml` runs the harnesses on `ubuntu-latest` (push touching `fuzz/**` → 60 s per target, weekly cron → 300 s, `workflow_dispatch` → any duration). Reach for `gh workflow run fuzz.yml -f duration=1800 -f targets=fuzz_css_parser` instead of rebuilding a local sanitizer setup. **WSL now works — the "WSL is dead" half of this gotcha is obsolete since 2026-08-19.** The user installed the toolchain there after freeing up `C:`; verified the same day: `rustc 1.100.0-nightly`, `cargo-fuzz 0.13.2`, `clang 18.1.3`, 950 GB free on `/`. The earlier blockers (`rustup` failing on an `llvm-tools-preview` / `llvm-tools` file conflict over `bin/llc`, a 100 %-full `C:` keeping `ext4.vhdx` from growing and the root filesystem read-only) are all gone. So a local run is available again — recipe and measured timings in [`fuzz/README.md`](fuzz/README.md) — and CI is no longer the *only* way, just the unattended one. **Native Windows remains impossible** for the PE/COFF reason above; that part of this gotcha stands.

When you discover a non-obvious implementation detail in a specific subsystem, add it to [`subsystems/<crate>.md`](subsystems/) under the relevant crate section (in English), not here.

---

## When in doubt

- **Architecture / scope** — `docs/plan/architecture.md` (§1 Principles, §3 Architecture).
- **Dependency policy** — `docs/plan/tech-stack.md` (§5).
- **How to build / run** — `README.md`.
- **Current code state** — `git log --oneline`.
- **Why a decision was made** — `docs/decisions/ADR-*.md` or `DECISIONS.md` (historical).

If the question isn't answered by these sources — ask the user, don't assume.
