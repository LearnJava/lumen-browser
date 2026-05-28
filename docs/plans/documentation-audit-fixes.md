# Documentation Audit — Fixes Plan

**Date:** 2026-05-28  
**Status:** IN PROGRESS

---

## Phase 1: Fix STATUS-PN.md line references (CRITICAL)

### 1.1 Fix STATUS-P1.md references to lumen-plan.md

| Task | Current | Correct | Fix |
|------|---------|---------|-----|
| extras-p2 | lumen-plan.md:195 | lumen-plan.md:231 | CHANGE |
| (all other P1 Queue tasks) | TBD | TBD | TBD |

**Action:** 
- [ ] Find all lumen-plan.md references in STATUS-P1.md
- [ ] Verify they point to correct task descriptions
- [ ] Update with correct line numbers

### 1.2 Fix STATUS-P2.md references to Queue tasks

**Issue:** STATUS-P2.md is near-empty (P2 role inactive); Queue tasks should either:
- Link directly to lumen-plan.md (better)
- Or be removed if no P2 developer exists yet

**Action:**
- [ ] Decide: Keep Queue in STATUS-P2.md or move to STATUS-P1.md?
- [ ] If keep: Add correct lumen-plan.md:line references
- [ ] If move: Consolidate P2 Queue into P1 Queue with clear ownership

### 1.3 Fix STATUS-P3.md references

**Current issues:**
- References to `§6.11`, `§11.4`, `§15` (don't exist in standard format)
- Ссылки на STATUS-P*.md without line numbers
- Some tasks already completed (marked ✅ but still listed)

**Action:**
- [ ] Replace paragraph references with ADR files (docs/decisions/ADR-*.md)
- [ ] Add line numbers or replace with full inline descriptions
- [ ] Remove completed tasks

---

## Phase 2: Fix CLAUDE.md lumen-plan.md references

### 2.1 Consolidate "Do not read lumen-plan.md" rule

**Current contradiction:**
- CLAUDE.md:205, 237: "Do not read lumen-plan.md unless..."
- CLAUDE.md:340, 346, 576: ссылаются на lumen-plan.md §1, §5, §12 как critical

**Fix:**
- [ ] Rewrite CLAUDE.md:205, 237 to clarify:
  - ✅ DO read: Principles (§1), Dependency policy (§5), Unique features (§12)
  - ❌ Don't read: Detailed roadmap tables (use STATUS-PN.md instead)
  - ❌ Don't read: Implementation history (use git log instead)

### 2.2 Duplicate critical sections from lumen-plan.md into CLAUDE.md

**Sections to copy:**
- [ ] Principles (§1) — move to CLAUDE.md §1
- [ ] Dependency policy (§5) — move to CLAUDE.md §5 (or separate section)
- Keep: Unique features (§12) as reference only

**Benefit:** Developers find critical info without opening lumen-plan.md

### 2.3 Replace grep examples with alternatives

**Current (bad):**
```bash
grep "P1.*⬜\|P1.*🟡" lumen-plan.md  # Contradicts "don't read lumen-plan"
```

**New (good):**
```bash
grep "Next step" STATUS-P1.md  # Find current task
grep "OPEN" BUGS.md             # Find bugs
```

---

## Phase 3: Resolve STATUS.md and deprecated files

### 3.1 STATUS.md decision

**Options:**
1. Delete (consolidate into STATUS-PN.md)
2. Keep as dashboard (aggregate current from STATUS-PN.md files)
3. Convert to navigation index

**Decision needed:** What to do?

### 3.2 Mark DECISIONS.md as deprecated

- [ ] Add warning banner at top
- [ ] Link to docs/decisions/

**Status:** ✅ DONE

---

## Phase 4: Consolidate duplicate documentation

### 4.1 Commands section (README.md vs CLAUDE.md)

**Issue:** Duplicated `cargo` commands in two places  
**Fix:**
- [ ] Keep README.md (user-facing, Russian)
- [ ] Keep CLAUDE.md (developer-facing, English)
- [ ] Add cross-references instead of duplication

### 4.2 Dependency policy (3 locations)

**Locations:**
1. CLAUDE.md §348-353 (summary)
2. lumen-plan.md §5 (full tables)
3. DECISIONS.md (historical context)

**Fix:**
- [ ] CLAUDE.md keeps summary
- [ ] lumen-plan.md keeps full tables (reference)
- [ ] DECISIONS.md marked as historical

---

## Implementation Order

1. **CRITICAL:** Fix STATUS-P1.md line references (extras-p2 and others)
2. **CRITICAL:** Fix STATUS-P2.md/P3.md references (or consolidate)
3. **HIGH:** Fix CLAUDE.md "don't read lumen-plan" contradiction
4. **HIGH:** Duplicate Principles + Dependency policy into CLAUDE.md
5. **MEDIUM:** Decide on STATUS.md fate
6. **MEDIUM:** Consolidate duplicated commands
7. **LOW:** Update cross-references where appropriate

---

## Notes

- All line numbers must be verified before and after edits
- Each edit should include: "docs: update references after [change]"
- STATUS-PN.md updates must be in same commit as referenced docs changes
