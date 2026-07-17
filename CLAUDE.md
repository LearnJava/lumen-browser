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
| `STATUS-P1.md` | P1 sprint: in-progress task, next items, recent merge. Read at session start if you are P1. |
| `STATUS-P2.md` | P2 — **реактивирована 2026-07-13** для задачи P2-wpt (WPT-интеграция через wptrunner+BiDi, `docs/tasks/p2-wpt-integration.md`, срезы S1–S8). Read at session start if you are P2. |
| `STATUS-P3.md` | P3 sprint: in-progress task, next items, recent merge. Read at session start if you are P3. |
| `STATUS-P4.md` | P4 sprint: CSS spec compliance. Read at session start if you are P4. |
| `STATUS-P5.md` | P5 maintenance: code-health aliases + sweep workflow. Read at session start if you are P5. |
| `lumen-plan.md` | TOC index: links to 11 section files in `docs/plan/`. Read for architecture; for daily status use `STATUS-PN.md` instead. |
| `docs/plan/` | Design doc split into 11 files: architecture, tech-stack, engine, web-apis-shell, privacy, features, knowledge, security-performance, testing, phases, meta. (The former `roadmap.md`/`history.md` were deleted 2026-07-02 — task status lives in `ROADMAP.md`, chronology in `git log`.) |
| `CSS-SPECS.md` | Complete CSS property & spec roadmap: all W3C modules, per-property status (✅🟡⬜🚫), P4 priority queue. |
| `docs/build-speed.md` | Compile-time optimization plan: current baseline, measurement protocol (S1–S5), ranked measures (stable / nightly / rejected), benchmark journal. Read before changing build config (profiles, `.cargo/config.toml`, sccache). |
| `docs/automation.md` | **All automation/introspection surfaces of the browser and when to apply them** (dump modes, `--deterministic`, MCP tools/resources, BiDi, IPC, driver-API, `LUMEN_NO_*` paint-bisect flags, known stubs). Read before writing a debugging script or a new test harness — the capability usually already exists. |
| `docs/roadmap-trees.md` | **How to use the interactive roadmap trees** (`docs/roadmap-*.html`): open in a browser, filters/search, and how to keep them current (`ROADMAP.md` + `python scripts/gen_roadmap.py`, auto-pulls bug status from `BUGS.md`). |
| `ROADMAP.md` | Flat, grep-friendly source of the phase/task tree (two markdown tables: phases + tasks, one task per line). Feeds `gen_roadmap.py`; replaced the old nested `docs/roadmap.json`. Bug↔task links live in its `bugs` column; CSS-module status is live-aggregated from `CSS-SPECS.md` into rows `css-specs-t0`…`t4` (note = `AUTO:CSS-SPECS:T<N>`, do not hand-edit that note). |
| `CLAUDE.md` | (this file) Conventions and invariants for the assistant. |
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

| Developer | Domain | Crates |
|---|---|---|
| **P1** | Feature development: any subsystem from roadmap (source → layout → paint → shell) | All crates (coordinated with P2/P4) |
| **P2** | **Reactivated 2026-07-13**: leads P2-wpt (WPT via `wptrunner` + WebDriver BiDi, `docs/tasks/p2-wpt-integration.md`) and the DEVX track (dev-tooling on existing automation surfaces, `docs/automation.md`, ROADMAP.md DEVX-1…6, assigned 2026-07-16). Was reserve (since 2026-06-18). | `lumen-bidi-server`, `lumen-driver`/`lumen-mcp` (DEVX-5), Python tooling `tests/wpt/` + `graphic_tests/run.py` (DEVX-1/4) |
| **P3** | **Bug fixes ONLY**: BUGS.md OPEN items, graphic test regressions | All crates (read-only except bug fixes) |
| **P4** | **CSS properties ONLY**: parsing, ComputedStyle, cascade, end-to-end wiring | `css-parser`, `layout` (style.rs), `paint` (display_list.rs) |
| **P5** | **Code health ONLY**: audit, workspace-clippy, stub/branch/docs/dep sweeps, safe mechanical cleanup | All crates (read-only except trivial clippy fixes in own crate + branch/worktree/SYMBOLS.md cleanup) |

**Task reservation:** create the `p<N>-<id>` branch — its existence is the reservation signal. A parallel session sees it via `git branch` and skips the task. Details — `docs/dev-roles.md`.

---

## Project Skills

5 skills in `.claude/skills/`. Use them instead of following protocols manually:

| Skill | When to use |
|---|---|
| `/lumen-add-css-property` | Adding a new CSS property to `lumen-layout` |
| `/lumen-task-start <name>` | Starting a new roadmap task (creates worktree + reserves in plan) |
| `/lumen-task-finish <name>` | Task ready to merge (clippy → tests → merge --no-ff → worktree remove) |
| `/lumen-new-crate <name>` | Creating a new Cargo crate in the workspace |
| `/lumen-health-check [target]` | P5 maintenance sweep (`full`/`clippy`/`stubs`/`branches`/`docs`/`deps`/`dupes`) |

`lumen-task-start` — explicit invocation only (`/`).
`lumen-add-css-property`, `lumen-new-crate`, `lumen-health-check`, and `lumen-task-finish` — Claude may invoke automatically from context.

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
1. Read `STATUS-PN.md` — pointer lines to open tasks; `git branch` shows any `p<N>-…` task in progress
2. Run `git branch` — verify you're on main
3. Architecture context → `docs/plan/architecture.md` §1, §3; decisions → `docs/decisions/README.md`

**Cargo output rules:** always `-p <crate>`, never `--workspace` (exception: P5). Success → 1 line. Errors → full `error[...]` block, skip all warnings. Test failure → test name + first 10 lines.

**Run discipline (details in `docs/commands.md`):** one cargo run — one log file (`> .tmp/<name>.log 2>&1`, then grep the file; never re-run cargo just to re-filter output). During iteration `cargo check -p` only; one `clippy -p` + targeted tests before the commit; full gates (workspace clippy + scoped-test) run exactly once inside `/lumen-task-finish`, synchronously in the foreground — never as background tasks.

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

**Every session MUST work in its own `git worktree`** — path `.claude/worktrees/<task-name>/`. Remove immediately after merge.

**7-step completion checklist** (all mandatory, full details in `docs/git-workflow.md`):
1. `cargo clippy -p <crate> -- -D warnings` + `cargo test -p <crate>`
2. `git merge --no-ff p<N>-task-name -m "Merge …"`
3. `git branch -d p<N>-task-name`
4. Delete pointer line from `STATUS-PN.md`, commit
5. `git push origin main`
6. `git worktree remove .claude/worktrees/<task-name>`

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
- **Parallel sessions in the same working tree = disaster.** Two sessions doing `git checkout` of different branches causes git to stash one session's work. Recovery via `git stash pop` is fragile. **Solution: mandatory `git worktree`s** (see Worktree isolation above). If you find yourself on a foreign branch — check `git stash list` before running `git restore .`.
- **Live-window BiDi/MCP `script.evaluate` can hang indefinitely in some working sessions.** Spawning `lumen --bidi-port <port>` and driving it (no other flags — plain window mode) sometimes never finishes installing the live window's JS runtime: every `script.evaluate` reports `"JS context not available"` forever, well past the normal install race that `LumenTestharnessExecutor`/`tests/wpt/verify_devx6_bidi_scenarios.py` already tolerate with a bounded retry. Reproduced independent of page content/URL scheme (`http://127.0.0.1:<port>/…`) and independent of which BiDi command triggered the eval — looks like a class of gap similar to the known `ffmpeg -f gdigrab` access-denied issue in some Windows sessions (DEVX-1/DEVX-4 notes), not something to root-cause inside an unrelated task. `browsingContext.navigate` itself (`DocumentReady`) and all non-eval BiDi commands are unaffected — only `script.evaluate`-dependent automation (`click`/`type` synthetic-event dispatch, `eval`, anything polling a JS global) is at risk. If you hit this, don't burn time chasing it inside your task — bound your `script.evaluate` retries and report/skip, same pattern as `verify_devx6_bidi_scenarios.py`'s `JsContextUnavailable`.

When you discover a non-obvious implementation detail in a specific subsystem, add it to [`subsystems/<crate>.md`](subsystems/) under the relevant crate section (in English), not here.

---

## When in doubt

- **Architecture / scope** — `docs/plan/architecture.md` (§1 Principles, §3 Architecture).
- **Dependency policy** — `docs/plan/tech-stack.md` (§5).
- **How to build / run** — `README.md`.
- **Current code state** — `git log --oneline`.
- **Why a decision was made** — `docs/decisions/ADR-*.md` or `DECISIONS.md` (historical).

If the question isn't answered by these sources — ask the user, don't assume.
