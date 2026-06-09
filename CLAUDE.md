# CLAUDE.md

Project context for Claude Code. Auto-loaded each session. Keeps the assistant oriented without re-asking questions answerable from code or adjacent docs.

**This file is English-only.** All edits — including gotchas added by other sessions — must be written in English. Translate before committing.

Update this file whenever you change architecture, invariants, or policies.

---

## What is this

**Lumen** — private, lightweight, transparent browser in Rust with a custom engine. Not a Chromium/WebKit wrapper; a standalone rendering engine with an embedded JS engine.

Current phase: **Phase 0 (prototype)**. Goal: open local HTML+CSS and render it via own pipeline. Status: `samples/page.html` opens, backgrounds and text render via bundled Inter.

| File | Contents |
|---|---|
| `README.md` | User-facing: install, commands, what to expect. |
| `STATUS-P1.md` | P1 sprint: in-progress task, next items, recent merge. Read at session start if you are P1. |
| `STATUS-P2.md` | P2 sprint: in-progress task, next items, recent merge. Read at session start if you are P2. |
| `STATUS-P3.md` | P3 sprint: in-progress task, next items, recent merge. Read at session start if you are P3. |
| `STATUS-P4.md` | P4 sprint: CSS spec compliance. Read at session start if you are P4. |
| `STATUS-P5.md` | P5 maintenance: code-health aliases + sweep workflow. Read at session start if you are P5. |
| `lumen-plan.md` | Full design doc (~2400 lines, 22 chapters): principles, scope, architecture, phases, roadmap, implementation history. Read for architecture/history; for daily status use `STATUS-PN.md` instead. |
| `CSS-SPECS.md` | Complete CSS property & spec roadmap: all W3C modules, per-property status (✅🟡⬜🚫), P4 priority queue. |
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

Five parallel developers (5 Claude Code sessions, each in its own `git worktree`). Each owns a distinct domain:

**If the user says "you are developer N" at session start — read `STATUS-PN.md` and take the first item from "Next". If "In progress" is set — continue that task. If all your tasks are taken — ask the user which task to take next.**

Crates: `shell` | `core` | `dom` `html-parser` `css-parser` `layout` `paint` `font` `encoding` `image` | `network` `storage` `knowledge` `bench`

| Developer | Domain | Crates |
|---|---|---|
| **P1** | Feature development: any subsystem from roadmap (source → layout → paint → shell) | All crates (coordinated with P2/P4) |
| **P2** | Feature development: any subsystem from roadmap (source → layout → paint → shell) | All crates (coordinated with P1/P4) |
| **P3** | **Bug fixes ONLY**: BUGS.md OPEN items, graphic test regressions | All crates (read-only except bug fixes) |
| **P4** | **CSS properties ONLY**: parsing, ComputedStyle, cascade, end-to-end wiring | `css-parser`, `layout` (style.rs), `paint` (display_list.rs) |
| **P5** | **Code health ONLY**: audit, workspace-clippy, stub/branch/docs/dep sweeps, safe mechanical cleanup | All crates (read-only except trivial clippy fixes in own crate + branch/worktree/SYMBOLS.md cleanup) |

### Feature development: P1 and P2 collaboration

**P1 and P2 work on features from the roadmap in parallel.** Coordination:
- **Before starting:** Check `STATUS-P1.md` and `STATUS-P2.md` to avoid duplicate task pickup
- **When reserving a task:** Update your `STATUS-PN.md` first (add "In progress" with branch name)
- **Cross-domain work** (layout + paint): Use separate branches, coordinate via commit messages
- **Crate conflicts:** Check git branches before touching a crate. If conflict, one session delays start

### Bug ownership: P3 only

**P1, P2, P4 do not fix bugs.** When discovering a bug while working:

1. Add a line to `BUGS.md` as `OPEN` with the next BUG-NNN number
2. Optionally add a `// BUG-NNN` comment at the call site
3. Continue the current feature task — do not context-switch

**P3 workflow:**
1. Run `python graphic_tests/run.py --continue-on-fail` → identify failing tests
2. Pick highest-deviation OPEN item from `BUGS.md`
3. Locate code via `SYMBOLS.md` + targeted grep (do not read whole files)
4. Fix + add regression test + mark `BUGS.md`: `OPEN → FIXED <date>`
5. `cargo clippy -p <crate> --all-targets -- -D warnings` → `cargo test -p <crate>` → commit

P3 branch prefix: `p3-bug-<id>`, e.g. `p3-bug023-opacity`.

### CSS ownership: P4 only

**P1, P2, P3 do not implement CSS properties.** All CSS work belongs to P4:

- CSS parsing (`css-parser`) — P4
- `ComputedStyle` fields and `apply_declaration()` — P4
- `var()` substitution, `@layer` ordering, cascade — P4
- Wiring stored values to layout algorithms — P4
- Wiring stored values to paint/display-list — P4
- CSS at-rules: `@media`, `@keyframes`, `@container`, `@layer`, `@supports` — P4

**P1/P2 write algorithm stubs for P4 to wire.** When a new layout or render primitive is needed:

1. P1/P2 implements the algorithm / GPU primitive
2. Expose a clean Rust interface (function or trait)
3. Add `// CSS: <property-name>` comment marking where P4 should connect
4. **Do not** add the property to `ComputedStyle` or `apply_declaration()` — P4's job

Example split for `float`:
```
P1 writes:  fn lay_out_with_floats(node, floats: &FloatContext)  // CSS: float, clear
P4 writes:  ComputedStyle.float field + apply_declaration("float") + calls lay_out_with_floats
```

Example split for `filter`:
```
P2 writes:  fn apply_filter_pass(cmd: FilterCommand)  // CSS: filter, backdrop-filter
P4 writes:  ComputedStyle.filter field + apply_declaration("filter") + emits FilterCommand
```

**P4 workflow:** When P1/P2 add a `// CSS: <property>` comment, P4 picks it up from `STATUS-P4.md` "Needs wiring" section. P4 implements end-to-end, then moves to "Recent". Async workflow — no pre-coordination required.

### Code health: P5 only

**P5 is the periodic-maintenance role.** It does NOT write features (P1/P2), does NOT fix visual/geometry bugs (P3), does NOT implement CSS (P4). P5 keeps the codebase healthy across the noise generated by four parallel sessions.

P5's mandate is **audit + cheap safe cleanup + filing tasks** — never solo refactoring that changes API or behaviour.

- **Use the `/lumen-health-check <target>` skill** instead of running sweeps by hand. Targets: `full` (default), `clippy`, `stubs`, `branches`, `docs`, `deps`, `dupes`. Trigger phrases and the full alias→action table live in `STATUS-P5.md`.
- **What P5 fixes directly (no separate task):** delete `--merged` branches, prune orphaned worktrees, regenerate `SYMBOLS.md`, trivial clippy fixes (unused import, stray `&`) **in its own crate only**.
- **What P5 does NOT fix — it files instead:**
  - visual/logic bug → `OPEN` line in `BUGS.md` (next BUG-NNN)
  - duplication / needed refactor → "Next" item in `STATUS-P1.md`/`STATUS-P2.md` with `file:line`
  - dangling `// CSS:` handoff → "Needs wiring" item in `STATUS-P4.md`
- **Workspace-clippy exception.** P5 is the only role allowed `cargo clippy --workspace` (see Cargo output rules) — the full sweep is its whole point; it catches cross-crate breakage that per-crate checks hide after multi-session merges.
- **Branch prefix:** `p5-health-<date>` or `p5-<topic>`. Worktree mandatory, same as P1–P4.

Full role definition: `STATUS-P5.md`.

### Collaboration rules

- **Crate ownership.** P1 stays out of `lumen-paint` without P2 agreement; P3 stays out of layout without P1 agreement. Reduces conflicts, doesn't block review.
- **`lumen-core` is shared.** P3 usually owns `lumen-core::ext` traits, but P1/P2 can add their own traits (e.g. `FontProvider`, `AccessibilityProvider`) without waiting. Coordinate via commit message.
- **`lumen-shell` is P3's.** Only P3 integrates into the shell. P1/P2 describe integration points in commit body; P3 picks them up as separate tasks.
- **Interface-first.** Cross-team tasks start with the owner publishing **types/traits** (with `todo!()` stubs) in a dedicated commit. Consumers implement against the stub; the real impl is a drop-in replacement.
- **Add extension points yourself.** Don't block on "P3 hasn't added the trait yet" — add it yourself, P3 reviews post-factum.
- **P1/P2/P3 → P4 handoff.** When a new algorithm needs a CSS property, add `// CSS: <property>` comment at the call site and note it in `STATUS-P4.md` under "Needs wiring". Do not wait for P4 — ship the algorithm, P4 wires CSS independently.

### Reserving a task

Create a feature branch (`git checkout -b <name>`) → in the **first commit on that branch** update `STATUS-PN.md`:

```
In progress: <task name>  branch: <branch-name>
Next step: <what to do first>  <file.rs:line>
```

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

`lumen-task-start` and `lumen-task-finish` — explicit invocation only (`/`).
`lumen-add-css-property`, `lumen-new-crate`, and `lumen-health-check` — Claude may invoke automatically from context.

---

## Commands

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

### Token efficiency rules

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

**lumen-plan.md reading rule:**
- **DO read if you need:** Principles (§1, 8 items), Dependency policy (§5, tables), Unique features (§12), Architecture decisions (docs/decisions/ADR-*.md)
- **DON'T read:** Detailed roadmap tables (use `STATUS-PN.md` instead) · Implementation history (use `git log` instead) · Task queue (use `STATUS-PN.md`)

**Grep instead of reading whole files.** Use targeted grep before opening large files:

```bash
# Open tasks in any crate:
grep "OPEN" BUGS.md

# Find bugs by ID:
grep "BUG-042" BUGS.md

# Find symbol by name:
grep "LayoutBox" SYMBOLS.md
```

**SYMBOLS.md — symbol index.** `SYMBOLS.md` is an auto-generated index of every `pub fn/struct/enum/trait/type` with `file:line` and first `///` doc line. Use it instead of reading source files to locate a symbol:

```bash
# Find where LayoutBox is defined:
grep "LayoutBox" SYMBOLS.md

# See all public items in lumen-paint:
grep -A 300 "^## lumen-paint" SYMBOLS.md | grep -m 300 "^\`"

# Find all public traits in the codebase:
grep "**trait**" SYMBOLS.md
```

Then read only the target lines: `Read file offset=<line> limit=30`. This replaces reading an entire file just to find a function signature.

Regenerate after adding/moving/renaming any public symbol: `python scripts/gen_symbols.py`. Add the updated `SYMBOLS.md` to the same commit as the code change.

**Session start protocol.** At the beginning of each session:
1. Read `STATUS-PN.md` (your developer number) — current "In progress" task
2. Run `git branch` — verify you're on main
3. If you need architecture context: read `lumen-plan.md` §1 (Principles) and §5 (Dependency policy)
4. If you need architectural decisions: read `docs/decisions/README.md` index

### Cargo output rules

Always use `-p <crate>`, never `--workspace`. **Exception: P5** (code-health role) may run `cargo clippy --workspace` as part of its periodic sweep — that full pass is the role's purpose. No other role uses `--workspace`.

- **Success** — one line: `cargo check OK`, `Clippy clean`, `All tests passed (23/23)`.
- **Build/clippy failure** — show each full `error[...]` block (message + file:line + code + help lines), skip all `warning[...]` blocks entirely.
- **Test failure** — show test name + first 10 lines of panic output, skip the rest.

### Detecting the OS at session start

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

### PATH note (Windows + Git Bash)

`cargo` is at `C:\Users\konstantin\.cargo\bin`. Git Bash on this machine does **not** pick it up automatically. Add before any `cargo` command:

```bash
export PATH="/c/Users/konstantin/.cargo/bin:$PATH"
```

Not needed in cmd / PowerShell — PATH is correct there.

---

## Graphic tests

`graphic_tests/NN-*.html` — 22 pages (00 calibration + 01-20 properties + `1000000-final.html`), one visual effect each, viewport 1024×720. Graphics only, no text.

**00-calibration.html** — required first test: magenta stripes (`#ff00ff`) 1024 px wide at top and bottom of body. Used to detect crop offset in the Lumen desktop screenshot.

**Magenta frame in all tests.** Each test page 01+ uses a 1px magenta frame around the full 1024×720 viewport. Pattern:

```html
<style>
  body { background: #ff00ff; width: 1024px; height: 720px; }
  .__f { background: <PAGE_BG>; width: 1022px; height: 718px; margin: 1px; padding: <PADDING>; overflow: hidden; }
</style>
<body>
  <div class="__f">
    <!-- all content here -->
  </div>
</body>
```

The 1px magenta body background shows through `.__f`'s margins on all 4 sides. Crop offset comes from TEST-00 (calibration), not from this frame. Trigger phrases: "find bugs from screenshots", "run graphic_tests".

### Running

```bash
python graphic_tests/run.py                          # blocking pipeline: first fail = stop
python graphic_tests/run.py --only 03                # single test
python graphic_tests/run.py --continue-on-fail       # diagnostic: run all, collect all results
python graphic_tests/run.py --recheck                # re-run only FAIL tests from latest.json
python graphic_tests/run.py --build                  # cargo build --release first, then run
python graphic_tests/run.py --no-cache               # force re-capture Edge screenshots
```

Results are saved to `graphic_tests/results/`:
- `YYYYMMDD-HHMMSS.json` — full results: status, diff%, diff_region bounding box per test
- `YYYYMMDD-HHMMSS.html` — visual report: Edge | Lumen | Diff images side by side for each FAIL
- `latest.json` — always points to the last run (used by `--recheck`)

Edge screenshots are cached: re-captured only when the HTML source is newer than the PNG.

Pipeline: build Lumen release (if needed), then for each test — Edge headless + Lumen gdigrab + crop by magenta marker + pixel diff + % threshold. First test exceeding threshold stops the pipeline.

Output is one line per test:
```
TEST-03: PASS (0.2%)
TEST-07: FAIL (18.4%) ← pipeline stopped here
```

### Rule: adding a new CSS property

In the **same commit** as the implementation:

1. Add object(s) to the relevant test in series `02–20` (or create a new file if not covered).
2. Add a demo to `graphic_tests/1000000-final.html`.
3. Update `graphic_tests/COVERAGE.md` — add a row for the property.
4. If creating a new test file — use the magenta frame pattern: `body { background: #ff00ff; }` + `.__f` wrapper div with `margin: 1px; width: 1022px; height: 718px; background: <PAGE_BG>;`. See "Magenta frame in all tests" above.
5. Add an entry to `TESTS` in `graphic_tests/run.py`.

Current coverage — `graphic_tests/COVERAGE.md`.

### Run rules

1. **No screenshots in the repo.** `graphic_tests/screenshots/*.png` are work artifacts — do not commit. Only the updated [`BUGS.md`](BUGS.md) goes in.
2. **A bug is only a visually noticeable artifact.** Non-zero pixels in `NN-diff.png` alone are not a bug. Skip if only visible under pixel-by-pixel inspection.
3. **Ignore text for now.** Glyph antialiasing will always diverge from Edge — not tracked until a dedicated task. Text-box geometry, padding/margin around text, line-height — that's layout, check as normal.
4. **Never rewrite test pages to work around engine limitations.** Test pages are the ground truth — they represent correct CSS as Edge renders it. If a test fails, fix the engine, not the test. Simplifying HTML to make a test pass is a false positive: the engine didn't improve, the bar was lowered. The only valid reason to edit a test page is a bug in the test itself (wrong expected output).
5. **Single tracker — `BUGS.md` in the repo root.** One line per bug, compact format:
   ```
   BUG-018 | OPEN  | inline padding wrong on nested divs | layout/src/flow.rs:312
   BUG-003 | FIXED 2026-05-10 | composite glyphs missing | font/src/parser.rs:201
   ```
   New bug: append with next number (current tail: BUG-022). Fixed: change `OPEN` → `FIXED <date>`, do not delete. WONTFIX: stays in file as-is.

### 8A.6 migration — COMPLETE (2026-05-31)

The Python/gdigrab/Edge pipeline (`graphic_tests/run.py`) remains a **nightly CI job** (edge-comparison gate). The primary test gate is now:

```bash
cargo test -p lumen-driver --features cpu-render   # CPU snapshot gate — all 57 pages
cargo test -p lumen-driver                          # structural assertions — 50 tests
```

**Completed subsystems of `cpu_raster` (all display-list primitives):**
FillRect / FillRoundedRect / DrawBorder / DrawOutline / DrawLinearGradient / DrawRadialGradient /
DrawConicGradient (libm-free `atan2` approx for cross-OS bit-identity) / DrawSvgPath / DrawImage
(grey placeholder) / DrawText (bundled Inter via `lumen_font::Rasterizer`) / PushClipRect + PopClip /
PushScrollLayer + PopScrollLayer / PushOpacity + PopOpacity / PushTransform + PopTransform (bilinear
resample of page-space layer through affine) / PushBlendMode + PopBlendMode (all 16 CSS modes) /
PushFilter + PopFilter (Gaussian blur + 7 colour filters) / PushBackdropFilter + PopBackdropFilter /
PushMaskLinearGradient + PushMaskRadialGradient + PushMaskConicGradient + PushMaskImage + PopMask.

`PAGES` in `snapshot_cpu.rs` covers all 57 html files in `graphic_tests/`. Regenerate references:
`SAVE_CPU_SNAPSHOTS=1 cargo test -p lumen-driver --features cpu-render`.

Next: 8A.7 (shell as first BrowserSession client, Phase 4) — not scheduled.

---

## Architecture

Dependency graph and crate scope — in [lumen-plan.md](lumen-plan.md). Direction: `lumen-core` → dom/font/parsers → layout → paint → shell. No cycles.

### Extension traits (`lumen-core::ext`)

**Defined:** `NetworkTransport`, `StorageBackend`, `SearchProvider`, `FilterListSource`, `RequestFilter`, `EncodingDetector`, `EventSink`, `DnsResolver`, `HstsEnforcement`, `HttpCredentialProvider`, `FontProvider`, `JsRuntime` (`NullJsRuntime` stub), `JsFetchProvider`, `ClipboardProvider` (`navigator.clipboard`; process-global, installed via `lumen_js::set_clipboard_provider`; shell impl `PlatformClipboard` in `shell/src/platform/clipboard.rs` — Win32 / wl-copy+xclip / pbcopy), `CredentialProvider` (WebAuthn / passkeys backing `navigator.credentials`; process-global via `lumen_js::set_credential_provider`; impl `lumen_network::VirtualAuthenticator` — software ES256 passkey store), `JsWebSocketProvider` / `JsWebSocketSession` / `JsWsEvent`, `BrowserSession` (ADR-006, `core/src/ext.rs:1514`), `IdbBackend` (`lumen-storage::indexed_db`), `SwBackend` (`core/src/ext.rs:1530`; per-origin SW registration JSON snapshot persistence; impl `lumen_storage::SwStore`), `MemoryPressureSource` + `MemoryPressureLevel` (ADR-008 §10H, `core/src/ext.rs` + `core/src/memory_pressure.rs`; Win32/Linux/macOS platform impls; `NullMemoryPressureSource` for tests), `EvictableCache` + `CacheRegistry` (ADR-008 §10D.3, `core/src/ext.rs`; implemented by `GlyphAtlas`, `ImageDecodeCache`, `LayerCache`; P3 shell wires `CacheRegistry::broadcast_pressure()` to `MemoryPressureSource` poll loop).

**Sprint 0 stubs:** `UnicodeProvider`, `IdnaProvider`, `PublicSuffixList`, `ContentDecoder` (`UnsupportedContentDecoder`), `FontFormat`, `SpellChecker`, `HyphenationProvider`.

**Defined (Phase 2):** `KnowledgeStore` (`core/src/ext.rs`, impl `lumen-knowledge::DefaultKnowledgeStore` — §12.1–12.4; shell @notes omnibox wiring).

**Planned:** `WindowingBackend`, `RenderBackend`, `TlsBackend`, `AiBackend`.

---

## Principles

Full list (8 items) — [lumen-plan.md](lumen-plan.md) §1.

---

## Dependency policy

Full tables (permanent + provisional + Lumen core) — [lumen-plan.md](lumen-plan.md) §5.

### No new dep without justification

Every new `[dependencies]` entry requires this in the commit body:

> **Why this dependency:** \<category (permanent / provisional), trait-anchor, graduation criterion if provisional\>

---

## Code conventions

### Rust version and edition

- **Rust 1.95+ stable**, pinned in `rust-toolchain.toml`.
- **Edition 2024**, resolver "3".
- MSVC toolchain on Windows.

### Style

- `dev` profile uses `opt-level = 1` for own code (10% slower build, 5-10× faster layout/paint) and `opt-level = 3` for deps via `[profile.dev.package."*"]` (wgpu/winit/rustls are unusable in pure debug; rationale in [DECISIONS.md](DECISIONS.md)).
- `clippy::all` + `clippy::pedantic` not yet enabled globally, but `cargo clippy -p <crate> --all-targets -- -D warnings` must pass before every commit.
- No unnecessary comments — only when explaining *why*, not *what*.
- **`///` doc comments on all public structs, fields, and functions are mandatory.** Parallel sessions rely on these to understand semantics without reading the full implementation. At minimum: what the value represents, what coordinate system or box model it uses, what units, what it includes/excludes. Example: `/// Border-box rectangle: includes padding + border, excludes margin.`
- Names: `snake_case` functions/fields, `PascalCase` types, `SCREAMING_SNAKE` constants.

### Tests-first for parsers and algorithms

Write tests before code for parsers (`html-parser`, `css-parser`, `font`) and algorithms (rasterizer, layout).

**Integration tests on real data are mandatory.** Unit tests on synthetic TTF bytes passed, but the `hhea` parser bug (skip 16 instead of 22) was only caught by an integration test on bundled Inter. Synthetic data does not replace reality.

### Error handling

- User-facing API: `Result<T, E>` with a meaningful `Error` enum.
- Internal: `Option` where `None` means "not found" / "not applicable" (not an error).
- No `panic!` / `unwrap()` in production code; allowed in tests.
- FFI boundaries (wgpu, future V8): `unsafe` isolated in one module, documented, reviewed.

### `unsafe` policy

- Forbidden outside FFI boundaries.
- Every `unsafe` block requires a `// SAFETY:` comment.

---

## Git workflow

### Branches

**All work happens in feature branches. Direct commits to `main` are forbidden.**

```bash
git checkout -b text-rendering
# ... commits ...
git checkout main
git merge --no-ff text-rendering -m "Merge text-rendering: ..."
git branch -d text-rendering
```

**`--no-ff` is required** — preserves "this commit series = one task" structure in `git log --graph`.

Branch names: short kebab-case. **Developer sessions (P1–P4) must prefix the branch name with their number:** `p1-text-rendering`, `p2-font-atlas`, `p3-http-client`, `p4-css-filter`. This makes it possible to identify which session owns a branch if it crashes mid-task.

### Commits

- **One logical step = one commit.** Don't batch unrelated changes.
- **Before commit:** at minimum `cargo check` must pass. Prefer full tests + clippy.
- **Commit message in Russian.** Short subject (under 80 chars), blank line, body explains *why* (not *what* — that's in the diff).
- **Trailer always at the end:**
  ```
  Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
  ```
- **Stage specific files** (`git add path1 path2`), not `git add -A` / `.` — prevents accidental inclusion of secrets or archives.

### Forbidden

- **Any commit directly to `main`** — including docs, "minor fixes", coordination notes.
- Force-push to `main`.
- Rewriting published history.
- `git config` changes (never).
- Skipping hooks (`--no-verify`).
- `git push` without explicit user request.

### Parallel session coordination

Multiple Claude Code sessions may work simultaneously. Full workflow for task lifecycle:

**Step 1: Task startup (BEFORE coding)**
1. Read `STATUS-PN.md` + `git branch` — check what's in progress
2. If "In progress" is already set — that task is taken, pick from "Next" instead
3. Create a feature branch and worktree: `git worktree add .claude/worktrees/<task-name> -b p<N>-task-name`
4. In the **first commit**, update `STATUS-PN.md`: set "In progress: <task>" + branch name + next step
5. Push the branch: `git push origin p<N>-task-name`

**Step 2: During work** — see "Worktree isolation" section below

**Step 3: Task completion (7 mandatory steps)** — see "Task completion checklist" section below

**If work is cancelled:**
- Delete the worktree: `git worktree remove .claude/worktrees/<task-name>`
- Delete the branch: `git branch -D p<N>-task-name`
- In a cleanup commit, remove the line from `STATUS-PN.md`
- Push: `git push origin main`

#### Worktree isolation — mandatory

**Every parallel Claude Code session MUST work in its own `git worktree`.**

```bash
git worktree add .claude/worktrees/<task-name> -b <branch-name>
```

Path must be inside the browser folder. Worktrees outside (`../lumen-<task>/`) are forbidden. After merge:

```bash
git worktree remove .claude/worktrees/<task-name>
```

Two sessions doing `git checkout` in the same directory causes git to stash one session's work — recovery via `git stash pop` is fragile.

#### Forbidden in shared working tree

- `git checkout <foreign-branch>` with uncommitted changes. Commit (`git commit -am "wip"`) or stash first.
- If accidentally on a foreign branch: do **not** run `git restore .` — check `git stash list` first, restore with `git stash pop`, then switch back.

#### Defensive WIP commits

Before any long pause (debug, test run, large multi-file edit) — `git commit -am "wip: <description>"` on your branch. Protects against process crashes and accidental stash collisions.

Before merge, squash wip commits with `git rebase -i HEAD~N` — only while the branch hasn't been pulled by another session.

#### Never leave a worktree on `main` with uncommitted/staged changes

A `main` worktree is a **temporary construct for atomic merge**. Remove it immediately after merge:

```bash
git worktree remove <path>
```

A dirty `main` worktree blocks all other sessions — git refuses `checkout main` with `fatal: 'main' is already used by worktree at <path>`.

**Zombie worktree** (path doesn't match branch, e.g. `.claude/worktrees/css-foo/` on `[main]`): `git -C <path> checkout -B zombie-stale-wip && git -C <path> commit -m "wip"` — frees main. Full procedure with patch archive — `.claude/docs/zombie-worktree.md`.

#### Task completion checklist (7 steps, all mandatory)

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
# — remove line from "In progress"
# — move task to "Recent"
git add STATUS-PN.md
git commit -m "P<N>: отметить task-name как завершённую"

# 5. Push to remote
git push origin main

# 6. Exit worktree and delete it (CRITICAL — blocks other sessions if left behind)
git worktree remove .claude/worktrees/<task-name>
# (session automatically returns to original directory)
```

**Why all 7 are mandatory:** Skipping delete-branch (step 3) or delete-worktree (step 6) leaves stale branches that accumulate. Skipping STATUS update (step 4) loses task history. Both cause confusion in parallel sessions and merge conflicts. As of 2026-05-28, 37 stale branches had accumulated due to incomplete cleanup. Commit to all 7 steps every time.

---

## Communication

- **Reply language: Russian.** The user speaks Russian.
- **Tone: technical, no emoji** unless the user uses them.
- **Brief and direct.** Short answer + what was done. No marketing text.
- **Files as clickable links:** `[lumen-plan.md](lumen-plan.md)`, `[crates/engine/font/src/rasterizer.rs:48](crates/engine/font/src/rasterizer.rs)`.

### Banned words

"Wikipedia" / "Википедия" — user explicitly asked not to use. Say "reference article", "external article", "external page" instead.

---

## Keep implementation status current

Update `lumen-plan.md`, the relevant `subsystems/<crate>.md`, and `CLAUDE.md` **in the same commit** as the implementation — not separately.

### `lumen-plan.md`

Header has the **"Implementation Status"** block; §16 has per-task markers. Legend: ✅ done · 🟡 in progress / partial · ⬜ planned.

After implementation: change ⬜ → ✅ (or 🟡 → ✅). If split — use 🟡 with a note.

### Related files

On significant milestones update:

- **[subsystems/\<crate\>.md](subsystems/)** — extend the crate section (added to "Done" / removed from "Deferred" / test count).
- **`lumen-plan.md` → Roadmap** — remove completed items.
- **[docs/decisions/](docs/decisions/)** — new architectural decision (new dep exception, API approach choice). Use TEMPLATE.md, update README.md index.
- **CLAUDE.md → "Known gotchas"** — if a gotcha is resolved or a new one is found.

No manual doc update needed for: typos, formatting, minor refactors without API changes, tests not changing crate capability, code comments, merge history.

---

## Subsystem state

Per-crate state (scope, done, deferred, invariants) — [SUBSYSTEMS.md](SUBSYSTEMS.md) (index) → `subsystems/<crate>.md`. Update the relevant crate file on every plan-item commit.

---

## Decisions log

**New decisions** — one ADR file per decision in [`docs/decisions/`](docs/decisions/), using the template at [`docs/decisions/TEMPLATE.md`](docs/decisions/TEMPLATE.md). Update the index table in [`docs/decisions/README.md`](docs/decisions/README.md).

**Historical decisions** (pre-ADR format) — [`DECISIONS.md`](DECISIONS.md). Do not add new entries there.

---

## Unique features (§12)

Full list with phases — [lumen-plan.md](lumen-plan.md) §12.

---

## Known gotchas

- **Cargo.lock is committed** (workspace includes a binary).
- **Line endings:** `.gitattributes` enforces LF. Git warning about CRLF→LF is normal.
- **Archives in repo root are gitignored** (`/*.zip`, `/*.tar*`). Downloaded files won't accidentally get committed.
- **Parallel sessions in the same working tree = disaster.** Two sessions doing `git checkout` of different branches causes git to stash one session's work. Recovery via `git stash pop` is fragile. **Solution: mandatory `git worktree`s** (see Worktree isolation above). If you find yourself on a foreign branch — check `git stash list` before running `git restore .`.

When you discover a non-obvious implementation detail in a specific subsystem, add it to [`subsystems/<crate>.md`](subsystems/) under the relevant crate section (in English), not here.

---

## When in doubt

- **Architecture / scope** — `lumen-plan.md`.
- **How to build / run** — `README.md`.
- **Current code state** — `git log --oneline` or status block in the plan.
- **Why a decision was made** — code comments or commit messages.

If the question isn't answered by these sources — ask the user, don't assume.
