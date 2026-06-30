# Commands reference

## Core commands

```bash
# Fast check (no linking) — 1-2 sec.
cargo check -p lumen-layout

# Tests for a specific crate.
cargo test -p lumen-font

# Integration tests on bundled Inter.
cargo test -p lumen-font --test inter_real_font

# Strict clippy (warnings = errors). Required before every commit.
cargo clippy -p lumen-layout --all-targets -- -D warnings

# Run browser with test page.
cargo run -p lumen-shell -- samples/page.html

# Empty window.
cargo run -p lumen-shell

# Headless dump modes (no winit / wgpu). Result to stdout, diagnostics to stderr.
cargo run -p lumen-shell -- --dump-source samples/page.html
cargo run -p lumen-shell -- --dump-layout samples/page.html
cargo run -p lumen-shell -- --dump-display-list samples/page.html

# ASCII glyph rasterization preview.
cargo run --example preview -p lumen-font

# Pipeline benchmark (decode → parse → layout → paint). Default 100 iters; override with LUMEN_BENCH_ITERS=...
cargo run -p lumen-bench --release
```

---

## Token efficiency rules

**One task — one session.** Start a new chat for each task. Long sessions accumulate context — every message costs more as the session grows. Use `/compact` if the session grew large but the task isn't finished yet.

**No verification reads after edits.** Don't ask to re-read a file after Edit/Write — the tool reports failure if something went wrong. Verify correctness with `cargo check`, not by re-reading.

**Precise task descriptions upfront.** Before describing a bug or task, grep/read to find the exact location first. Include file:line so the next session doesn't re-search:

```
crates/engine/layout/src/style.rs:248 — compute_style,
margin: auto doesn't account for containing block width
```

**Use dump modes before reading source.** 5 lines of dump output often replace reading 3-4 source files:

```bash
# layout bugs (box model, margin, padding, inline flow):
cargo run -p lumen-shell -- --dump-layout samples/page.html 2>&1 | grep -A2 "margin\|padding"

# paint/rendering bugs (colors, order, display list):
cargo run -p lumen-shell -- --dump-display-list samples/page.html 2>&1 | grep -A2 "FillRect\|Text"
```

**"What can the browser do?" → read `CAPABILITIES.md` only.** It is the single source of truth for shipped capability, verified against code and grouped by subsystem. Do NOT re-read `docs/plan/phases.md`, `lumen-plan.md`, or `STATUS-PN.md` for this — they track *intent* and *task queues* and drift from code. Keep `CAPABILITIES.md` true to code: update it in the same commit as any feature merge.

**docs/plan/ reading rule:**
- **DO read if you need:** `docs/plan/architecture.md` (§1 Principles, §5 Dependency policy), `docs/plan/knowledge.md` (§12 Unique features), `docs/decisions/ADR-*.md`
- **DON'T read:** `docs/plan/roadmap.md` (use `STATUS-PN.md` instead) · `docs/plan/history.md` (use `git log` instead) · `docs/plan/phases.md` / `lumen-plan.md` markers (use `CAPABILITIES.md` for what's done)

**Grep instead of reading whole files.** Use targeted grep before opening large files:

```bash
# Open tasks in any crate:
grep "OPEN" BUGS.md

# Find bugs by ID:
grep "BUG-042" BUGS.md

# Find symbol by name:
grep "LayoutBox" SYMBOLS.md
```

**SYMBOLS.md — symbol index.** Auto-generated index of every `pub fn/struct/enum/trait/type` with `file:line`. `grep "SymbolName" SYMBOLS.md` → `Read file offset=<line> limit=30`. Regenerate on every public API change: `python scripts/gen_symbols.py` (add to same commit).

---

## Cargo output rules

Always use `-p <crate>`, never `--workspace`. **Exception: P5** (code-health role) may run `cargo clippy --workspace` as part of its periodic sweep — that full pass is the role's purpose. No other role uses `--workspace`.

- **Success** — one line: `cargo check OK`, `Clippy clean`, `All tests passed (23/23)`.
- **Build/clippy failure** — show each full `error[...]` block (message + file:line + code + help lines), skip all `warning[...]` blocks entirely.
- **Test failure** — show test name + first 10 lines of panic output, skip the rest.

---

## Detecting the OS at session start

Run this once at the beginning of each session to know which OS you're on:

```bash
uname -s 2>/dev/null || echo "Windows"
```

- Output starts with `Linux` → you're on Linux (CI, WSL, remote server).
- Output is `Windows` or the command fails → you're on Windows (Git Bash, MSVC toolchain).

Behaviour that differs by OS:

| | Windows (Git Bash) | Linux |
|---|---|---|
| `cargo` PATH | needs `export PATH="/c/Users/konstantin/.cargo/bin:$PATH"` | available by default |
| worktree paths | `D:/RustProjects/lumen-browser/.claude/worktrees/…` | `/path/to/lumen-browser/.claude/worktrees/…` |
| screenshot tool | `ffmpeg` gdigrab (see `utils/`) | not available; skip graphic tests |
| child process tracking | full (orchestrator) | limited — no auto window open, use tmux |

---

## PATH note (Windows + Git Bash)

`cargo` is at `C:\Users\konstantin\.cargo\bin`. Git Bash on this machine does **not** pick it up automatically. Add before any `cargo` command:

```bash
export PATH="/c/Users/konstantin/.cargo/bin:$PATH"
```

Not needed in cmd / PowerShell — PATH is correct there.
