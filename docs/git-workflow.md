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

**Step 1: Task startup (BEFORE coding)**
1. Read `STATUS-PN.md` + `git branch` — check which tasks already have a `p<N>-…` branch
2. If a `p<N>-…` branch already exists for the task — it's taken, pick the next pointer line instead
3. Create a feature branch and worktree: `git worktree add .claude/worktrees/<task-name> -b p<N>-task-name`
4. Push the branch: `git push origin p<N>-task-name` — its existence reserves the task (the STATUS pointer line stays in place)

**Step 2: During work** — see "Worktree isolation" section below

**Step 3: Task completion (7 mandatory steps)** — see "Task completion checklist" section below

**If work is cancelled:**
- Delete the worktree: `git worktree remove .claude/worktrees/<task-name>`
- Delete the branch: `git branch -D p<N>-task-name`
- In a cleanup commit, remove the line from `STATUS-PN.md`
- Push: `git push origin main`

---

## Worktree isolation — mandatory

**Every parallel Claude Code session MUST work in its own `git worktree`.**

```bash
git worktree add .claude/worktrees/<task-name> -b <branch-name>
```

Path must be inside the browser folder. Worktrees outside (`../lumen-<task>/`) are forbidden. After merge:

```bash
git worktree remove .claude/worktrees/<task-name>
```

Two sessions doing `git checkout` in the same directory causes git to stash one session's work — recovery via `git stash pop` is fragile.

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

## Task completion checklist (7 steps, all mandatory)

**After task is done and ready to merge, execute ALL 7 steps in order. Missing even one step causes accumulated stale branches.**

```bash
# 1. Verify code is production-ready
cargo clippy -p <crate> -- -D warnings
cargo test -p <crate>

# 2. Merge branch to main with --no-ff
git checkout main
git merge --no-ff p<N>-task-name -m "Merge p<N>-task-name: описание"

# 3. Delete branch immediately after merge
git branch -d p<N>-task-name

# 4. Update STATUS-PN.md on main
# — delete the completed task's pointer line (history lives in git log)
git add STATUS-PN.md
git commit -m "P<N>: отметить task-name как завершённую"

# 5. Push to remote
git push origin main

# 6. Exit worktree and delete it (CRITICAL — blocks other sessions if left behind)
git worktree remove .claude/worktrees/<task-name>
# (session automatically returns to original directory)
```

**Why all 7 are mandatory:** Skipping delete-branch (step 3) or delete-worktree (step 6) leaves stale branches that accumulate. Skipping STATUS update (step 4) loses task history. Both cause confusion in parallel sessions and merge conflicts. As of 2026-05-28, 37 stale branches had accumulated due to incomplete cleanup. Commit to all 7 steps every time.
