# Git workflow

## Branches

**All work happens in feature branches. Direct commits to `main` are forbidden.**

```bash
git checkout -b text-rendering
# ... commits ...
git checkout main
git merge --no-ff text-rendering -m "Merge text-rendering: ..."
git branch -d text-rendering
```

**`--no-ff` is required** — preserves "this commit series = one task" structure in `git log --graph`.

Branch names: short kebab-case. **Developer sessions (P1–P5) must prefix the branch name with their number:** `p1-text-rendering`, `p2-font-atlas`, `p3-http-client`, `p4-css-filter`. This makes it possible to identify which session owns a branch if it crashes mid-task.

---

## Commits

- **One logical step = one commit.** Don't batch unrelated changes.
- **Before commit:** at minimum `cargo check` must pass. Prefer full tests + clippy.
- **Commit message in Russian.** Short subject (under 80 chars), blank line, body explains *why* (not *what* — that's in the diff).
- **Trailer always at the end:**
  ```
  Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
  ```
- **Stage specific files** (`git add path1 path2`), not `git add -A` / `.` — prevents accidental inclusion of secrets or archives.

---

## Forbidden

- **Any commit directly to `main`** — including docs, "minor fixes", coordination notes.
- Force-push to `main`.
- Rewriting published history.
- `git config` changes (never).
- Skipping hooks (`--no-verify`).
- `git push` without explicit user request.

---

## Parallel session coordination

Multiple Claude Code sessions may work simultaneously. Full workflow for task lifecycle:

**Step 0: Sync with the remote (BEFORE step 1)**
1. `git pull origin main` on the `main` worktree, before reading `STATUS-PN.md`/`ROADMAP.md` or creating any branch/worktree. Parallel sessions push to `origin/main` independently, so a session that skips this reads stale task-pointer/roadmap state and, if left long enough, turns what would have been a small conflict into a large multi-file one.
2. If `pull` fast-forwards or auto-merges cleanly — done, proceed to Step 1.
3. If `pull` reports real conflicts (diverged history, `<<<<<<<` markers): resolve **file-by-file**, reading enough context on both sides to understand intent — do not run `git checkout --ours/--theirs` blindly across many files, and do not let an agent resolve a whole batch without spot-checking cross-file consistency (shared globals, renamed identifiers, feature-flag names). After resolving, re-verify: `cargo check`/`cargo clippy -- -D warnings` for every touched crate, plus scoped `cargo test` for crates with real logic conflicts (not just doc/config files) — a clean compile does not prove a merge is behaviorally correct. Only then commit the merge.
4. Push the resolved `main` immediately (`git push origin main`) so other sessions see it before it can drift again.

**Step 1: Task startup (BEFORE coding)**
1. Read `STATUS-PN.md` + `git branch` — check which tasks already have a `p<N>-…` branch
2. If a `p<N>-…` branch already exists for the task — it's taken, pick the next pointer line instead
3. Occupy your pool slot with the task branch: `cd "$(bash scripts/worktree-pool.sh p<N>-work p<N>-task-name | tail -1)"` — see "Worktree isolation" below
4. Push the branch: `git push origin p<N>-task-name` — its existence reserves the task (the STATUS pointer line stays in place)

**Step 2: During work** — see "Worktree isolation" section below

**Step 3: Task completion (7 mandatory steps)** — see "Task completion checklist" section below. Step 5 of that checklist (`git push origin main`) is not optional or batchable — push right after the merge commit, in the same sitting, not at some later "wrap-up" point.

**If work is cancelled:**
- Free the slot: `bash scripts/worktree-pool.sh release p<N>-work` (it refuses while unmerged commits exist — that is the point; delete the branch below only if you really mean to drop that work)
- Delete the branch: `git branch -D p<N>-task-name`
- In a cleanup commit, remove the line from `STATUS-PN.md`
- Push: `git push origin main`

---

## Worktree isolation — mandatory

**Every parallel Claude Code session MUST work in its own `git worktree`.** Two sessions doing `git checkout` in the same directory causes git to stash one session's work — recovery via `git stash pop` is fragile.

**Use your persistent pool slot, not a fresh worktree per task:**

```bash
cd "$(bash scripts/worktree-pool.sh p<N>-work p<N>-task-name | tail -1)"
```

One slot per developer — `.claude/worktrees/p1-work` … `p5-work` (plus `perf-base` for A/B baselines). The slot is created once (~3 min: 62 291 files) and then reused: only the branch changes, `target/` stays warm. A fresh worktree per task costs the `add` **plus** a cold build of 9–15 min, because `git worktree remove` deletes the warm `target/` with the directory (`docs/build-speed.md` §7, scenario S3). Measured on the session of 2026-07-27: 15.5 min for the first `cargo test -p lumen-shell --no-run` in a fresh worktree, and another 22 min for a second fresh worktree built only to get a baseline binary of `main`.

This is **not** a shared `CARGO_TARGET_DIR` (rejected — `docs/build-speed.md` §6): each slot keeps its own `target/`, so the target lock never serializes parallel sessions.

Slot discipline:

- **Build only `dev-release` inside a slot.** Warm `dev-release` ≈ 4.7 GB, `debug` ≈ 9.4 GB, `release` ≈ 2.4 GB — five slots on `debug` would eat ~70 GB. `--release` is forbidden for tests anyway (2–3× slower).
- `bash scripts/worktree-pool.sh list` — see which slot holds which branch, whether it is dirty and whether its `target/` is warm.
- The script **refuses** to switch a slot that has uncommitted work or commits not yet merged into `main` — a crashed session's work is never wiped silently. Commit (`git -C <slot> commit -am "wip: ..."`) or merge first.
- An interrupted `git worktree add` (timed-out tool call) leaves `index.lock` without an `index` and a half-populated tree. The script detects exactly that case (lock present, index absent, no own commits) and repairs it with `reset --hard` instead of refusing.
- A slot cannot sit on `main` — the main working tree holds it. `release` therefore parks the slot on a detached HEAD at `main`.

Ad-hoc worktrees are still allowed for one-off needs (merge helpers, experiments); path must be inside the browser folder — `../lumen-<task>/` and `/tmp/...` are forbidden — and they must be removed with `git worktree remove` right after use.

### Safety rules in worktrees

Never `git checkout <foreign-branch>` with uncommitted changes — commit (`git commit -am "wip: ..."`) first. If accidentally on a wrong branch: check `git stash list` before `git restore .`, then `git stash pop` and switch back. Before any long pause — commit a wip: protects against crashes. Squash wip commits with `git rebase -i HEAD~N` before merge (only while branch hasn't been pulled).

### Never leave a worktree on `main` with uncommitted/staged changes

A `main` worktree is a **temporary construct for atomic merge**. Remove it immediately after merge:

```bash
git worktree remove <path>
```

A dirty `main` worktree blocks all other sessions — git refuses `checkout main` with `fatal: 'main' is already used by worktree at <path>`.

**Zombie worktree** (path doesn't match branch, e.g. `.claude/worktrees/css-foo/` on `[main]`): `git -C <path> checkout -B zombie-stale-wip && git -C <path> commit -m "wip"` — frees main. Full procedure with patch archive — `.claude/docs/zombie-worktree.md`.

---

## Task completion checklist (8 steps, all mandatory)

**After task is done and ready to merge, execute ALL 8 steps in order. Missing even one step causes accumulated stale branches.**

```bash
# 1. Verify code is production-ready
cargo clippy -p <crate> -- -D warnings
cargo test -p <crate>

# 1b. Push the task branch and wait for green CI (decided 2026-08-18,
#     docs/ci-offload.md §8 variant 1). `ci.yml`/`red-lines.yml` trigger on
#     `p[1-5]-*` pushes, so no pull request is needed.
git push -u origin p<N>-task-name
gh run watch --repo LearnJava/lumen-browser \
  "$(gh run list --repo LearnJava/lumen-browser --branch p<N>-task-name \
       --workflow CI --limit 1 --json databaseId -q '.[0].databaseId')"
# Red? Fix on the branch and push again — do NOT merge a red branch.
# `gh` lives at /c/Program Files/GitHub CLI and is not on the bash PATH by default.

# 2. Merge branch to main with --no-ff
git checkout main
git merge --no-ff p<N>-task-name -m "Merge p<N>-task-name: описание"

# 3. Free the pool slot, then delete the branch
#    (a slot still holding the branch makes `branch -d` fail:
#     "cannot delete branch ... used by worktree at ...")
bash scripts/worktree-pool.sh release p<N>-work
git branch -d p<N>-task-name

# 4. Update STATUS-PN.md on main
# — delete the completed task's pointer line (history lives in git log)
git add STATUS-PN.md
git commit -m "P<N>: отметить task-name как завершённую"

# 5. Push to remote
git push origin main

# 6. Pool slot: already freed in step 3 — the slot and its warm target/ stay.
#    Ad-hoc worktree (not a pool slot): delete it, it blocks other sessions.
git worktree remove .claude/worktrees/<task-name>

# 7. Delete the remote task branch pushed in step 1b — it has served its purpose.
git push origin --delete p<N>-task-name
```

**Why all 8 are mandatory:** Skipping delete-branch (step 3) or leaving an ad-hoc worktree behind (step 6) leaves stale branches and directories that accumulate. Skipping STATUS update (step 4) loses task history. Skipping step 7 leaves the remote littered — 29 stale remote branches had piled up by 2026-08-18, the oldest from June. Both cause confusion in parallel sessions and merge conflicts. As of 2026-05-28, 37 stale local branches had accumulated due to incomplete cleanup. Commit to all 8 steps every time.

### The local gate is NOT replaced by CI (yet)

`docs/ci-offload.md` §8 sketches moving the gate to CI. **That has not happened.** Today `ci.yml`
runs `cargo check -p lumen-shell` plus unit tests of 16 crates — it does **not** run
`clippy --workspace` and does not run the CPU snapshot tests, so it is strictly *weaker* than the
local gate. Step 1b is therefore an addition on top of `/lumen-task-finish`, not a replacement:
CI catches what the local gate cannot (Linux and macOS), the local gate catches what CI does not
(lints, snapshots, scoped tests). Only once the `lint` and `snapshot-cpu` jobs of
`docs/ci-offload.md` §4 exist and are green does trimming the local gate become a real question.
