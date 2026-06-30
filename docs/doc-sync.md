# Doc sync rules

Update docs **in the same commit** as the code change. Never update docs separately.
Do not re-read a whole file to make a small update — use `grep -n` to find the line, then targeted `Read offset=N limit=10` + `Edit`.

---

## Per change type

| Change type | Files to update | What exactly to do |
|---|---|---|
| New feature / capability | `CAPABILITIES.md` | `grep -n "<subsystem>\|<keyword>" CAPABILITIES.md` → change ⬜/🟡 → ✅ on that line |
| New feature / capability | `subsystems/<crate>.md` | append bullet to **Done** section (file is small — read whole) |
| New feature / capability | `STATUS-PN.md` (your role) | delete the completed task's pointer line |
| Bug fixed | `BUGS.md` | `grep -n "BUG-NNN" BUGS.md` → change `OPEN` → `FIXED <date>` |
| CSS property (P4) | `CSS-SPECS.md` | `grep -n "<property-name>" CSS-SPECS.md` → change ⬜ → ✅ |
| CSS property (P4) | `CAPABILITIES.md` | same as "New feature" above |
| New dependency | `docs/plan/tech-stack.md` | append row to the relevant table (permanent or provisional) |
| Architectural decision | `docs/decisions/ADR-NNN.md` | new file from TEMPLATE.md; update `docs/decisions/README.md` index |
| Known gotcha found/fixed | `CLAUDE.md` → "Known gotchas" | append/remove the bullet |
| New public API (`pub fn/struct`) | `SYMBOLS.md` | regenerate: `python scripts/gen_symbols.py` |
| Roadmap structure (phase/task) or bug status change | `ROADMAP.md` (structure + bug↔task links) → regenerate | edit `ROADMAP.md` if a phase/task/link changed (one task = one line, `grep "| U-6 " ROADMAP.md`), then run `python scripts/gen_roadmap.py` — it re-pulls live bug status from `BUGS.md` and inlines data into `docs/roadmap-*.html`. Bug-only status changes need just the script (no ROADMAP.md edit). |

---

## What NOT to update

- `lumen-plan.md` — 24-line TOC only, no status content
- `docs/plan/history.md` — deprecated stub (use `git log`)
- `docs/plan/roadmap.md` — historical reference, not a task tracker

---

## No doc update needed for

Typos, formatting, minor refactors without API changes, tests that don't change crate capability, code comments, merge commits.
