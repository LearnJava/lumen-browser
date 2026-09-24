# CLAUDE.md

Loaded in full at the start of every session, so it holds **routing and invariants only** — every line is paid for on every task. A rule that needs more than three lines, or that bites only in one area, goes into a linked file or a nested `CLAUDE.md` (`crates/js/`, `crates/engine/paint/`, `graphic_tests/` — Claude Code loads those only when you work there). English only. Size cap enforced by `scripts/check_claude_md.py`.

## What is this

**Lumen** — private, lightweight, transparent browser in Rust with a custom engine, not a Chromium/WebKit wrapper. Phase and version — [`docs/plan/phases.md`](docs/plan/phases.md).

- **Layering:** `lumen-core` → dom/font/parsers → layout → paint → shell. No cycles. All architectural invariants — [`docs/invariants.md`](docs/invariants.md).
- **V8 (`rusty_v8`) is the only JS engine**; `rquickjs` is gone from the workspace — never target it.
- **Never hardcode a version** — derive it from `CARGO_PKG_VERSION` (the one manual site: the `navigator.userAgent` literal in `crates/js/src/shim/web_api_shim_mid_b.js`).

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

Write only inside the repository and its worktrees under `.claude/worktrees/`. Nothing outside it — dotfiles, `~/.config`, sibling projects, `../lumen-*` worktrees: describe the external change and wait for approval. Exception: Claude memory under `~/.claude/projects/`.

## Session start and git

Protocol — [`docs/git-workflow.md`](docs/git-workflow.md). Closing a task — `/lumen-task-finish`; starting one — `/lumen-task-start` (explicit `/` invocation only).

1. `git pull origin main` first, before reading STATUS files or branching.
2. "You are developer N" → continue your existing `p<N>-…` branch if there is one, else take the **first** line of `STATUS-PN.md`. Strictly top-down: the order encodes dependencies. If line 1 cannot start, say why and ask.
3. Work in your pool slot: `cd "$(bash scripts/worktree-pool.sh p<N>-work p<N>-<task> | tail -1)"`. The branch is the reservation.
4. Every commit: gate → commit → `merge --no-ff` into `main` → push. Never commit on `main`; no force-push, history rewrite, `git config`, `--no-verify` (also enforced by `.claude/settings.json`). CI is not awaited — watch `main` after the push.
5. Commit message in Russian, subject under 80 chars, body says *why*. Stage explicit paths, never `git add -A`.

## Gates

- Before every commit: `cargo clippy -p <crate> --all-targets -- -D warnings`. Lints live in `[workspace.lints]`, rules in [`docs/conventions.md`](docs/conventions.md); a new crate needs `[lints] workspace = true`.
- Anything that can move pixels → full `python graphic_tests/run.py --continue-on-fail` and regenerated goldens **in the same commit** ([`docs/graphic-tests.md`](docs/graphic-tests.md)). Anything else → `scripts/scoped-test.sh` + `python graphic_tests/dump_golden.py`.
- A new dependency needs `**Why this dependency:** <permanent/provisional, trait-anchor, graduation criterion>` in the commit body ([ADR-027](docs/decisions/ADR-027-own-vs-vendored-boundary.md)).
- Live real-site testing runs `--maximized` with the ad-block off (`LUMEN_NO_ADBLOCK=1`) — [`docs/automation.md`](docs/automation.md) §Flags.
- Docs move with the code, in the same commit — [`docs/doc-sync.md`](docs/doc-sync.md).

## Communication

- **Reply in Russian**, technical tone, no emoji unless the user uses them, no marketing text. Files as clickable markdown links labelled `path:line`.
- Banned word: "Wikipedia" / "Википедия" — say "reference article" / "external page".

## Known gotchas — only traps that bite whatever the task is

- **Never round-trip a repo markdown file through Python text mode.** `BUGS.md` holds raw CR bytes in table cells; universal-newline reading splits rows and shifts every `STATUS-PN.md` pointer below. Use `newline=''` on read *and* write, or `Edit`/`sed`.
- **A fresh or interrupted worktree can have the whole repo staged as deleted** — `git status --short` before the first `git add`, `git diff --cached --stat` before every commit there ([`docs/git-workflow.md`](docs/git-workflow.md) §Worktree isolation).
- **sccache must be ≥ 0.17.0** — `.cargo/config.toml` wraps every compiler call in it, and an older one kills each `rustc`/`clippy-driver` with `0xc0000409`.

Area-specific traps go to `subsystems/<crate>.md`, `docs/engine-gaps.md` or a nested `CLAUDE.md`; a trap whose defect is fixed is deleted. **If no source answers the question — ask the user, don't assume.**
