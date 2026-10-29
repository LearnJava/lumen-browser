# AGENTS.md

Loaded in full at the start of every Kimi Code session, so it holds **routing and invariants only**. Load this file initially; open linked documents on demand, only when relevant to the current task. A rule that needs more than three lines, or that bites only in one area, goes into a linked file or a nested `CLAUDE.md`. English only.

## What is this

**Lumen** — private, lightweight, transparent browser in Rust with a custom engine, not a Chromium/WebKit wrapper. Phase and version — [`docs/plan/phases.md`](docs/plan/phases.md).

- **Layering:** `lumen-core` → dom/font/parsers → layout → paint → shell. No cycles. All architectural invariants — [`docs/invariants.md`](docs/invariants.md).
- **V8 (`rusty_v8`) is the only JS engine**; `rquickjs` is gone from the workspace — never target it.
- **Never hardcode a version** — derive it from `CARGO_PKG_VERSION`. The browser's name in every layer comes from one `lumen_core::Identity`; the Chrome major is `chrome_major!` (`crates/core/src/identity.rs`).

## Where to look

| Question | Source |
|---|---|
| What can the browser do | `CAPABILITIES.md` — **only** this |
| What should I work on | `STATUS-PN.md` (bare `<source>:NN` pointers) · roles — [`docs/dev-roles.md`](docs/dev-roles.md) |
| Open / fixed defects | `BUGS.md` · `BUGS-FIXED.md` · `bugs/BUG-NNN-*.md` |
| Before writing any probe | [`docs/engine-gaps.md`](docs/engine-gaps.md) · [`docs/probe-method.md`](docs/probe-method.md) |
| Commands and gate discipline | [`docs/commands.md`](docs/commands.md) |
| Everything else — git, doc-sync, lints, perf, WPT, CI, ADRs, subsystems | [`docs/INDEX.md`](docs/INDEX.md) |

## Working boundary

Write only inside the repository and its worktrees under `.claude/worktrees/`. Nothing outside it — dotfiles, `~/.config`, sibling projects, `../lumen-*` worktrees: describe the external change and wait for approval.

## Session start and git

Protocol — [`docs/git-workflow.md`](docs/git-workflow.md). When the user asks to start or finish a roadmap task, read `.claude/skills/lumen-task-start/SKILL.md` or `.claude/skills/lumen-task-finish/SKILL.md` and follow it as a manual protocol. These are not registered Kimi Code slash commands.

The Git protocol is not blanket authorization for mutations or remote operations: pull, branch, commit, merge, and push only within the scope requested by the user.

1. `git pull -q origin main` first, before reading STATUS files or branching.
2. "You are developer N" → continue your existing `p<N>-…` branch if there is one, else take the **first** line of `STATUS-PN.md`. Strictly top-down: the order encodes dependencies. If line 1 cannot start, say why and ask.
3. Take the task: `cd "$(bash scripts/task-claim.sh <N> <task-id> | tail -1)"` — fetches origin, refuses if any unmerged branch carries the task id, occupies the pool slot and pushes the branch. Only a branch **on origin** reserves.
4. Closing commit: gate → commit → `merge --no-ff` into `main` → push; a WIP commit (task unfinished) goes to the task branch only, `cargo check` instead of the gate. Never commit on `main`; no force-push, history rewrite, `git config`, `--no-verify`. CI is not awaited — watch `main` after the push.
5. Commit message in Russian, subject under 80 chars, body says *why*. Stage explicit paths, never `git add -A`. Use truthful authorship attribution; never copy a hardcoded Claude model trailer for work authored by Kimi.

## Gates

- Gate once, before the closing commit (not WIP): `cargo clippy -p <crate> --all-targets -- -D warnings` + targeted tests; while iterating `cargo check` only, `dev-release` only for a binary you need ([`docs/commands.md`](docs/commands.md) §Build cost). Lints live in `[workspace.lints]`, rules in [`docs/conventions.md`](docs/conventions.md); a new crate needs `[lints] workspace = true`.
- Anything that can move pixels → full `python graphic_tests/run.py --continue-on-fail` and regenerated goldens **in the same commit** ([`docs/graphic-tests.md`](docs/graphic-tests.md)). Anything else → `scripts/scoped-test.sh` + `python graphic_tests/dump_golden.py`.
- A new dependency needs `**Why this dependency:** <permanent/provisional, trait-anchor, graduation criterion>` in the commit body ([ADR-027](docs/decisions/ADR-027-own-vs-vendored-boundary.md)).
- Live real-site testing runs `--maximized` with the ad-block off (`LUMEN_NO_ADBLOCK=1`) — [`docs/automation.md`](docs/automation.md) §Flags.
- Docs move with the code, in the same commit — [`docs/doc-sync.md`](docs/doc-sync.md).

## Area-specific instructions

Nested `CLAUDE.md` files are not loaded automatically. Before working in an area they cover, read the relevant file explicitly:

- `crates/js/CLAUDE.md`
- `crates/engine/paint/CLAUDE.md`
- `graphic_tests/CLAUDE.md`

Find other task-specific protocols through [`docs/INDEX.md`](docs/INDEX.md).

## Communication

- **Reply in Russian**, technical tone, no emoji unless the user uses them, no marketing text. Files as clickable markdown links labelled `path:line`.
- Banned word: "Wikipedia" / "Википедия" — say "reference article" / "external page".
- End each turn with the task declared done or, when a decision is pending, a clear question through the available Kimi Code interaction flow. Do not emit Claude-only slash commands or fenced `ask-user` blocks as Kimi Code mechanisms.

## Known gotchas — only traps that bite whatever the task is

- **Never round-trip a repo markdown file through Python text mode.** Line-ending normalization can rewrite table rows and shift every `STATUS-PN.md` pointer below. Use `newline=''` on read *and* write, or `Edit`/`sed`.
- **A fresh or interrupted worktree can have the whole repo staged as deleted** — `git status --short` before the first `git add`, `git diff --cached --stat` before every commit there ([`docs/git-workflow.md`](docs/git-workflow.md) §Worktree isolation).
- **sccache must be ≥ 0.17.0** — `.cargo/config.toml` wraps every compiler call in it, and an older one kills each `rustc`/`clippy-driver` with `0xc0000409`.

Area-specific traps go to `subsystems/<crate>.md`, `docs/engine-gaps.md` or a nested `CLAUDE.md`; a trap whose defect is fixed is deleted. Keep shared rules in their canonical documents listed in `docs/INDEX.md`; do not duplicate them here. **If no source answers the question — ask the user, don't assume.**
