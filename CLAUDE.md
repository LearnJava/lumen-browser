# CLAUDE.md

Bootstrap context for Claude Code, loaded in full at the start of every session. **Routing and invariants only** — a rule that needs more than three lines to state belongs in a `docs/` file this one links to. Every line here is paid for on every task, whatever the task is.

**This file is English-only.** Translate before committing. Update it when a critical rule or a route changes; detail goes into the linked file, not here.

---

## What is this

**Lumen** — private, lightweight, transparent browser in Rust with a custom engine. Not a Chromium/WebKit wrapper. Phase 2 / **v0.5 «Interactive»** complete; phase↔version policy and history — [`docs/plan/phases.md`](docs/plan/phases.md).

- **Layering:** `lumen-core` → dom/font/parsers → layout → paint → shell. No cycles.
- **V8 (`rusty_v8`) is the only JS engine.** `rquickjs` is gone from the workspace — never target it with a fix or an investigation. The shared shim lives in `crates/js/src/shim/*.js` (edit the `.js`, not `dom.rs`); those files are read verbatim, so nothing in them is escaped. Read [`subsystems/js.md`](subsystems/js.md) before any JS/Web-API work — its §Invariants is the trap list.
- **Never hardcode a version** — every version string derives from `CARGO_PKG_VERSION`. The one manual site is the `navigator.userAgent` literal in `crates/js/src/shim/web_api_shim_mid_b.js`.

---

## Where to look

| Question | Source |
|---|---|
| What can the browser do right now | `CAPABILITIES.md` — **only** this, not `docs/plan/*` or `STATUS-PN.md` |
| What should I work on | `STATUS-PN.md` — bare `<source>:NN` pointer lines, nothing else |
| Open defects · fixed · per defect | `BUGS.md` · `BUGS-FIXED.md` · `bugs/BUG-NNN-*.md` |
| Task/phase tree | `ROADMAP.md` (one task = exactly one line) · viewers `docs/roadmap-*.html` |
| CSS property / spec status | `CSS-SPECS.md` |
| Architecture, principles | [`docs/plan/architecture.md`](docs/plan/architecture.md) §1, §3 · index [`lumen-plan.md`](lumen-plan.md) |
| What a local change must not break | [`docs/invariants.md`](docs/invariants.md) |
| Per-crate state, API and traps | [`SUBSYSTEMS.md`](SUBSYSTEMS.md) → `subsystems/<crate>.md` |
| Why a decision was made | [`docs/decisions/`](docs/decisions/) (ADRs) · `DECISIONS.md` (historical, read-only) |
| Commands, run and gate discipline | [`docs/commands.md`](docs/commands.md) |
| Automation surfaces (dumps, MCP, BiDi, IPC, env flags) | [`docs/automation.md`](docs/automation.md) |
| Git, worktree pool, task checklist | [`docs/git-workflow.md`](docs/git-workflow.md) |
| Which docs a change must update | [`docs/doc-sync.md`](docs/doc-sync.md) |
| Style, lints, grandfathered debt | [`docs/conventions.md`](docs/conventions.md) · [`docs/lint-policy.md`](docs/lint-policy.md) |
| Reviewing someone's diff | [`REVIEW.md`](REVIEW.md) |
| Pixel / graphic tests | [`docs/graphic-tests.md`](docs/graphic-tests.md) |
| How to probe, how to read a WPT failure | [`docs/probe-method.md`](docs/probe-method.md) |
| **Live engine gaps a probe will walk into** | [`docs/engine-gaps.md`](docs/engine-gaps.md) — read before writing any probe |
| How to measure and accept a perf change | [`docs/perf-method.md`](docs/perf-method.md) |
| WPT status and vendoring | `docs/wpt-status.md` · `tests/wpt/VENDOR.md` · `docs/wpt-vendor-notes/` |
| Build speed · CI | [`docs/build-speed.md`](docs/build-speed.md) · [`docs/ci-offload.md`](docs/ci-offload.md) |

---

## Working boundary

**Write only inside the browser folder** — `D:\RustProjects\lumen-browser\` and its worktrees in `.claude/worktrees/*`. Same for docs, configs, snapshot tests. Everything outside — `~/.bashrc`, `~/.config/*`, system dotfiles, sibling projects, ad-hoc worktrees like `../lumen-<task>/` — do not touch; if a task requires an external change, describe it and wait for approval. `git worktree add` obeys the same rule: the path must be under `.claude/worktrees/`.

Exception: Claude memory (`~/.claude/projects/.../memory/`) lives outside the repo by design.

---

## Roles and session start

Role definitions, ownership, collaboration, task-tracking schema — [`docs/dev-roles.md`](docs/dev-roles.md).

**"You are developer N"** → read `STATUS-PN.md` and take the **first** pointer line. If a `p<N>-…` branch already exists for you (`git branch`), continue that task instead. If everything is taken, ask which task to take next.

**Take the list strictly top-down** — never skip a line because a lower one looks smaller or clearer. The order encodes dependencies, not preference: a lower line is often the *symptom* of a higher one, so working out of order means fixing a symptom on top of a live defect and redoing it later. If the first line genuinely cannot be started (blocked by another role, needs a user decision), say so explicitly and ask.

**Task reservation** is the `p<N>-<id>` branch itself — a parallel session sees it via `git branch` and skips the task.

**Session start:**
1. **`git pull origin main` first**, before reading STATUS files or creating a branch. Parallel sessions push to `origin/main` independently, so local `main` lags; starting stale means stale pointers and one large conflict later instead of a small one now. If the pull reports a real diverged history, resolve it file-by-file understanding both sides — never blindly take one — and re-verify every touched crate before committing the merge.
2. Read `STATUS-PN.md`; `git branch` shows any `p<N>-…` task already in progress.
3. Occupy your pool slot ([`docs/git-workflow.md`](docs/git-workflow.md)) and confirm you are not working on `main`.

---

## Project skills

Use these instead of running the protocol by hand:

| Skill | When |
|---|---|
| `/lumen-add-css-property` | adding a CSS property to `lumen-layout` |
| `/lumen-task-start <name>` | starting a roadmap task — **explicit `/` invocation only** |
| `/lumen-task-finish <name>` | task ready to merge (gates → merge `--no-ff` → free the slot) |
| `/lumen-new-crate <name>` | new crate in the workspace |
| `/lumen-health-check [target]` | P5 maintenance sweep |
| `/lumen-perf-audit` | real-site performance audit (PERF track) |

All except `lumen-task-start` may be invoked automatically from context.

---

## Git

Full protocol, worktree pool, 7-step completion checklist — [`docs/git-workflow.md`](docs/git-workflow.md).

- **Direct commits to `main` are forbidden.** All work happens in `p<N>-<task-name>` branches (P1–P5 prefix mandatory), merged with `--no-ff`.
- **Every session works in its own worktree** — your persistent pool slot: `cd "$(bash scripts/worktree-pool.sh p<N>-work p<N>-task-name | tail -1)"`. Build only `dev-release` inside a slot.
- **Merge and push after EVERY commit** (user, 2026-08-19): gate → commit → `git merge --no-ff` into `main` → `git push origin main`. Nothing waits for the end of a task — unpushed work does not exist for the other sessions.
- **Do not wait for CI before merging.** The local gate is the only pre-merge check; watch `main` afterwards and fix it if CI goes red.
- Commit message in Russian, subject under 80 chars, body explains *why*.
- **Forbidden:** direct commit to main · force-push · rewriting history · `git config` · `--no-verify` · `git push` without an explicit user request.

---

## Code conventions

Full rules — [`docs/conventions.md`](docs/conventions.md). Lint status and the per-crate debt behind `#[allow]` — [`docs/lint-policy.md`](docs/lint-policy.md); a rule a lint can check belongs there, not in prose here.

- **Rust pinned to 1.97.0**, edition 2024, resolver "3", MSVC on Windows. **sccache must be ≥ 0.17.0** — an older one kills every `rustc`/`clippy-driver` invocation with `0xc0000409`.
- `cargo clippy -p <crate> --all-targets -- -D warnings` must pass before every commit. `-p <crate>` while working; `--workspace` only in the final gate (`/lumen-task-finish`) and P5's sweep — [`docs/commands.md`](docs/commands.md) §Cargo output rules.
- Machine-checked, so not negotiable: no `panic!`/`unwrap()`/`expect()` in production · `// SAFETY:` on every `unsafe` block (one comment does not cover two) · `///` on every public item · a new `.rs` file ≤ 2000 lines and a file already over it must not grow.
- **A new crate must carry `[lints] workspace = true`** or it silently escapes every project lint.
- Names: `snake_case` functions/fields, `PascalCase` types, `SCREAMING_SNAKE` constants.

**No new dependency without justification.** The boundary is decision ownership — ours where *we* decide what correct means, vendored where a committee already did ([ADR-027](docs/decisions/ADR-027-own-vs-vendored-boundary.md), tables in [`docs/plan/tech-stack.md`](docs/plan/tech-stack.md) §5). Every new `[dependencies]` entry requires this in the commit body:

> **Why this dependency:** \<category (permanent / provisional), trait-anchor, graduation criterion if provisional\>

---

## Testing gates

Details, flags, the debtor ratchet and how to regenerate each golden set — [`docs/graphic-tests.md`](docs/graphic-tests.md).

- **Anything that can move pixels** (paint/display list, layout geometry, a CSS property, font/text/image) needs the full `python graphic_tests/run.py --continue-on-fail` — ~20 min, foreground, focused window. Everything else is gated by `scripts/scoped-test.sh` + `python graphic_tests/dump_golden.py`; claiming display-list neutrality requires *showing* an empty `dump_golden.py` diff.
- **Three golden sets drift independently** — the Edge pixel diff, the deterministic CPU PNGs (`graphic_tests/snapshots/cpu/`) and the textual display-list snapshots (`crates/engine/paint/tests/snapshots/*.snap`). A paint change regenerates the affected ones **in the same commit**, or it red-lights someone else's gate later.
- **Live testing of real sites launches `--maximized`** (user, 2026-09-04) — a smaller window changes the CSS viewport, so it under-measures the page's real work and hides viewport-dependent defects. Exceptions: `graphic_tests/run.py` keeps its calibrated `--deterministic --viewport 1024x720` (every golden was captured there), and perf/census fixtures whose point is a fixed viewport — [`docs/automation.md`](docs/automation.md) §Flags.
- **Hard rules:** never edit a test page to work around an engine limit · never change a threshold (0.5 % for every test) · no screenshots committed.

---

## Communication

- **Reply in Russian.** Technical tone, no emoji unless the user uses them. Short answer + what was done; no marketing text.
- Files as clickable links: `[crates/engine/font/src/rasterizer.rs:48](crates/engine/font/src/rasterizer.rs)`.
- Banned word: "Wikipedia" / "Википедия" (user's explicit request) — say "reference article" / "external page".

---

## Known gotchas

**Live traps only, and only ones that bite regardless of what the task is.** Subsystem traps belong in `subsystems/<crate>.md`, probe traps in [`docs/engine-gaps.md`](docs/engine-gaps.md), method lessons in [`docs/probe-method.md`](docs/probe-method.md) / [`docs/perf-method.md`](docs/perf-method.md). A gotcha whose defect is fixed is **deleted**, not annotated — the narrative stays in `git log` and `bugs/BUG-NNN-FIXED.md`.

- **Parallel sessions in one working tree = disaster.** Two sessions checking out different branches makes git stash one session's work, and `stash pop` recovery is fragile. Hence the mandatory worktrees; if you find yourself on a foreign branch, check `git stash list` before `git restore .`.
- **An interrupted `git worktree add` leaves an index in which the whole repository is staged as deleted.** The checkout is ≈59 000 files, so a tool timeout hits it routinely, and the leftover `index.lock` makes a later `reset --hard` exit 0 without repairing anything — a commit made in that state deletes the tree. Run `git status --short` in a fresh worktree before the first `git add`, and check `git diff --cached --stat` before every commit made in one.
- **Never round-trip a repo markdown file through Python text mode.** `BUGS.md` holds raw CR bytes inside table cells; universal-newline reading turns each into a row split, which invalidates every `STATUS-PN.md` pointer below the damage (those are bare line numbers). Pass `newline=''` on **read** as well as write, or use binary mode / `Edit` / `sed`.
- **A newline inside a `ROADMAP.md` table cell silently truncates the whole task table** — `gen_roadmap.py` stops at the first line not starting with `|`, with no error; the trees just render fewer tasks. One task = exactly one line; verify with `grep -c '"id":' docs/roadmap-B-twotrees.html`.
- **`.ignore` (repo root) keeps ripgrep out of `.claude/worktrees/`** — those are full checkouts of other roles' branches, 75 % of all `.rs` files reachable from the root. Do not delete it; git does not read it, so it affects search only.
- **A stale `target/` after a toolchain change** surfaces as `E0786 "only metadata stub found for rlib dependency core"` — it reads like a broken dependency and is cured by `cargo clean`.
- **`cargo-fuzz`/libFuzzer cannot run on native Windows** — PE/COFF has no equivalent of the `__start___sancov_*` symbols libFuzzer needs, so this is structural. Use WSL ([`fuzz/README.md`](fuzz/README.md)) or `gh workflow run fuzz.yml`.
- **A `debug_assert!` is not a check in the profile everyone builds** — `dev-release` and `release` both compile it out, so a `debug_assert!` guarding a JS-visible spec requirement enforces nothing where it matters ([BUG-954](bugs/BUG-954-OPEN.md) is exactly this shape). Check whether it is the only guard before trusting one.
- **`--screenshot` (CPU, `cpu_raster.rs`) and the live window (wgpu, `renderer.rs`) are independent implementations of every `DisplayCommand`** — a match on one proves nothing about the other. And any paint/scroll number is meaningless without the wgpu backend it was measured on (`[wgpu] adapter: …` in stderr; pin it with `WGPU_BACKEND=vulkan|dx12|gl`).

When you discover a non-obvious detail in a specific subsystem, add it to `subsystems/<crate>.md` (in English), not here.

---

## When in doubt

Architecture/scope → [`docs/plan/architecture.md`](docs/plan/architecture.md) · dependency policy → [`docs/plan/tech-stack.md`](docs/plan/tech-stack.md) §5 · build and run → [`README.md`](README.md) · current code state → `git log --oneline` · why a decision was made → [`docs/decisions/`](docs/decisions/).

**If the question isn't answered by these sources — ask the user, don't assume.**
