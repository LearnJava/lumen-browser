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
| Bug fixed | `BUGS.md` → `BUGS-FIXED.md` + `bugs/BUG-NNN-*.md` | `grep -n "BUG-NNN" BUGS.md` → **move** the row into `BUGS-FIXED.md` with `FIXED <date>` (closed rows live in the archive since 2026-08-31 — 548 of 918 rows were closed ones, i.e. 2/3 of a file read in every session), rename `bugs/BUG-NNN-OPEN.md` → `-FIXED.md`, then `python scripts/remap_status_pointers.py --apply` — moving a row shifts every `STATUS-PN.md` pointer below it |
| Bug turns out to be a **feature gap**, not a defect | `BUGS.md` + `ROADMAP.md` + `bugs/BUG-NNN-OPEN.md` + `STATUS-P3.md` | status cell → `OPEN (ДОРАБОТКА → <task>)`; add the `ROADMAP.md` task row (bug id in its `bugs` column) and re-run `gen_roadmap.py`; add a `**Тип:**` line under the bug file's status; delete the pointer line from `STATUS-P3.md`. **Do not rename or move the bug file** — STATUS files and the Python tooling reference it by path, and the record of observations stays useful where it is |
| CSS property (P4) | `CSS-SPECS.md` | `grep -n "<property-name>" CSS-SPECS.md` → change ⬜ → ✅ |
| CSS property (P4) | `CAPABILITIES.md` | same as "New feature" above |
| New dependency | `docs/plan/tech-stack.md` | append row to the relevant table (permanent or provisional) |
| Architectural decision | `docs/decisions/ADR-NNN.md` | new file from TEMPLATE.md; update `docs/decisions/README.md` index |
| **Live** gotcha found (a trap you can still walk into) | the narrowest file that owns it | subsystem implementation detail → `subsystems/<crate>.md` · trap a probe will hit → `docs/engine-gaps.md` · test/gate trap → `docs/graphic-tests.md` · **only** if it bites regardless of the task → `CLAUDE.md` → "Known gotchas", one bullet, 1–3 lines, with the OPEN bug ref |
| Gotcha's defect **fixed** | wherever the bullet lives | **delete the bullet.** The narrative stays in `git log` + `bugs/BUG-NNN-FIXED.md`; a residual moves into the OPEN bug that tracks it. Do not rewrite it as "~~was~~ — fixed" — that is how `CLAUDE.md`'s gotcha section once reached 163 KB |
| Method lesson learned from a bug (how to probe, what counts as evidence) | `docs/probe-method.md` (perf → `docs/perf-method.md`) | append the rule; keep it about the method, not about the defect |
| Perf slice merged with a transferable lesson | `docs/perf-method.md` | append the rule (one paragraph, slice ref in brackets); per-slice numbers stay in `bugs/BUG-NNN-*.md` |
| New public API (`pub fn/struct`) | — | `SYMBOLS.md` is generated and **gitignored** (2026-08-31): nothing to commit. Regenerate locally when you use it: `python scripts/gen_symbols.py` |
| Roadmap structure (phase/task) or bug/CSS-module status change | `ROADMAP.md` (structure + bug↔task links) → regenerate | edit `ROADMAP.md` if a phase/task/link changed (one task = one line, `grep "| U-6 " ROADMAP.md`), then run `python scripts/gen_roadmap.py` — it re-pulls live bug status from `BUGS.md` and live CSS-module status from `CSS-SPECS.md` (rows `css-specs-t0`…`t4`), then inlines data into `docs/roadmap-*.html`. Those stay committed — they are hand-written viewers and the script only refills their `<script id="roadmap-data">` block, it cannot create the file — but since 2026-08-31 CI no longer compares them byte for byte, so a regeneration does not have to ride along in every commit. Bug-only or CSS-module-only status changes need no ROADMAP.md edit at all. |

---

## What NOT to update

- `lumen-plan.md` — short TOC only, no status content
- Implementation chronology — that is `git log`, not a doc (the former `docs/plan/history.md` / `docs/plan/roadmap.md` were deleted 2026-07-02; task tracking lives in `ROADMAP.md` + `STATUS-PN.md`)

---

## No doc update needed for

Typos, formatting, minor refactors without API changes, tests that don't change crate capability, code comments, merge commits.
