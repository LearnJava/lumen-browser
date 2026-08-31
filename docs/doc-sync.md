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
| Bug fixed | `BUGS.md` → `BUGS-FIXED.md` | `grep -n "BUG-NNN" BUGS.md` → **move** the row into `BUGS-FIXED.md` with `FIXED <date>` (closed rows live in the archive since 2026-08-31), then `python scripts/remap_status_pointers.py --apply` |
| CSS property (P4) | `CSS-SPECS.md` | `grep -n "<property-name>" CSS-SPECS.md` → change ⬜ → ✅ |
| CSS property (P4) | `CAPABILITIES.md` | same as "New feature" above |
| New dependency | `docs/plan/tech-stack.md` | append row to the relevant table (permanent or provisional) |
| Architectural decision | `docs/decisions/ADR-NNN.md` | new file from TEMPLATE.md; update `docs/decisions/README.md` index |
| Known gotcha found/fixed | `CLAUDE.md` → "Known gotchas" | append/remove the bullet |
| New public API (`pub fn/struct`) | — | `SYMBOLS.md` is generated and **gitignored** (2026-08-31): nothing to commit. Regenerate locally when you use it: `python scripts/gen_symbols.py` |
| Roadmap structure (phase/task) or bug/CSS-module status change | `ROADMAP.md` (structure + bug↔task links) → regenerate | edit `ROADMAP.md` if a phase/task/link changed (one task = one line, `grep "| U-6 " ROADMAP.md`), then run `python scripts/gen_roadmap.py` — it re-pulls live bug status from `BUGS.md` and live CSS-module status from `CSS-SPECS.md` (rows `css-specs-t0`…`t4`), then inlines data into `docs/roadmap-*.html`. Those stay committed — they are hand-written viewers and the script only refills their `<script id="roadmap-data">` block, it cannot create the file — but since 2026-08-31 CI no longer compares them byte for byte, so a regeneration does not have to ride along in every commit. Bug-only or CSS-module-only status changes need no ROADMAP.md edit at all. |

---

## What NOT to update

- `lumen-plan.md` — short TOC only, no status content
- Implementation chronology — that is `git log`, not a doc (the former `docs/plan/history.md` / `docs/plan/roadmap.md` were deleted 2026-07-02; task tracking lives in `ROADMAP.md` + `STATUS-PN.md`)

---

## No doc update needed for

Typos, formatting, minor refactors without API changes, tests that don't change crate capability, code comments, merge commits.
