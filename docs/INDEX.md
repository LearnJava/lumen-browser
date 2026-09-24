# Documentation index

The full routing table. `CLAUDE.md` keeps only the handful of routes every session needs; everything
else lives here so that it is read on demand, not paid for at every session start.

**One rule, one home.** Each row names the file that *owns* a topic. Other files may link to it but
must not restate it — a restated rule drifts, and the two copies then contradict each other.

## State of the project

| Question | Source |
|---|---|
| What can the browser do right now | `CAPABILITIES.md` — **only** this, not `docs/plan/*` or `STATUS-PN.md` |
| What should I work on | `STATUS-PN.md` — bare `<source>:NN` pointer lines, schema in [`dev-roles.md`](dev-roles.md) §Task tracking schema |
| Open defects · fixed · per defect | `BUGS.md` · `BUGS-FIXED.md` · `bugs/BUG-NNN-*.md` |
| Task/phase tree | `ROADMAP.md` (one task = exactly one line) · viewers `docs/roadmap-*.html` ([`roadmap-trees.md`](roadmap-trees.md)) |
| CSS property / spec status | `CSS-SPECS.md` |
| Bug priority rules | [`bug-priority.md`](bug-priority.md) |
| Per-crate state, API and traps | [`SUBSYSTEMS.md`](../SUBSYSTEMS.md) → `subsystems/<crate>.md` |
| WPT status and vendoring | [`wpt-status.md`](wpt-status.md) · `tests/wpt/VENDOR.md` · [`wpt-vendor-notes/`](wpt-vendor-notes/) |

## Architecture and decisions

| Question | Source |
|---|---|
| What a local change must not break | [`invariants.md`](invariants.md) |
| Architecture, principles, scope | [`plan/architecture.md`](plan/architecture.md) §1, §3 · plan index [`lumen-plan.md`](../lumen-plan.md) |
| Dependency policy (own vs vendored) | [`plan/tech-stack.md`](plan/tech-stack.md) §5 · [ADR-027](decisions/ADR-027-own-vs-vendored-boundary.md) |
| Why a decision was made | [`decisions/`](decisions/) (ADRs) · `DECISIONS.md` (historical, read-only) |
| Browser chrome (UI) architecture | [`shell-ui-architecture.md`](shell-ui-architecture.md) |

## Process

| Question | Source |
|---|---|
| Roles, ownership, task-tracking schema | [`dev-roles.md`](dev-roles.md) |
| Git, worktree pool, merging, task completion | [`git-workflow.md`](git-workflow.md) · skill `/lumen-task-finish` |
| Which docs a change must update | [`doc-sync.md`](doc-sync.md) |
| Style, toolchain, lints, grandfathered debt | [`conventions.md`](conventions.md) · [`lint-policy.md`](lint-policy.md) |
| Reviewing someone's diff | [`REVIEW.md`](../REVIEW.md) |
| Working efficiently as an agent (reads, cargo runs, background jobs) | [`commands.md`](commands.md) §Token efficiency rules |
| Parallel AI workers | [`ai-workers.md`](ai-workers.md) |

## Running, testing, measuring

| Question | Source |
|---|---|
| Commands, gate discipline, cargo output rules | [`commands.md`](commands.md) |
| Automation surfaces (dumps, MCP, BiDi, IPC, CLI flags, env toggles) | [`automation.md`](automation.md) |
| Pixel / graphic tests, the three golden sets | [`graphic-tests.md`](graphic-tests.md) |
| How to probe, how to read a WPT failure | [`probe-method.md`](probe-method.md) |
| Live engine gaps a probe will walk into | [`engine-gaps.md`](engine-gaps.md) — read before writing any probe |
| Conformance measurement | [`conformance-method.md`](conformance-method.md) |
| How to measure and accept a perf change | [`perf-method.md`](perf-method.md) · [`benchmarking-strategy.md`](benchmarking-strategy.md) · [`perf/`](perf/) |
| Testing an external site with Lumen | [`testing-your-site-with-lumen.md`](testing-your-site-with-lumen.md) |
| Fuzzing (Linux/WSL/CI only) | [`fuzz/README.md`](../fuzz/README.md) |
| Build speed · CI | [`build-speed.md`](build-speed.md) · [`ci-offload.md`](ci-offload.md) |
| Health-sweep history (P5) | [`HEALTH-LOG.md`](HEALTH-LOG.md) |

## Project skills (`.claude/skills/`)

| Skill | When |
|---|---|
| `/lumen-task-start <name>` | starting a roadmap task — **explicit `/` invocation only** |
| `/lumen-task-finish <name>` | task ready to merge: gates → doc-sync → merge `--no-ff` → push → free the slot |
| `/lumen-add-css-property` | adding a CSS property end to end |
| `/lumen-new-crate <name>` | new crate in the workspace |
| `/lumen-health-check [target]` | P5 maintenance sweep |
| `/lumen-perf-audit` | real-site performance audit (PERF track) |

## Nested `CLAUDE.md` files

Loaded by Claude Code only when a session reads files under that directory:

| File | Covers |
|---|---|
| [`crates/js/CLAUDE.md`](../crates/js/CLAUDE.md) | V8-only, the shared shim, per-feature shims |
| [`crates/engine/paint/CLAUDE.md`](../crates/engine/paint/CLAUDE.md) | CPU vs wgpu renderers, golden sets, backend-dependent numbers |
| [`graphic_tests/CLAUDE.md`](../graphic_tests/CLAUDE.md) | pipeline hard rules: foreground run, thresholds, test pages |
