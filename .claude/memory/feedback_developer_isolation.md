---
name: Developer isolation — never touch other developers' changes
description: When assigned as P1/P2/P3/P4/P5, do not touch, analyze, or commit changes from other developers
type: feedback
originSessionId: 57e8594b-433a-47dd-a245-9d53c650d71f
---
If working as a numbered developer (P1–P5): never touch, examine, or commit uncommitted changes from other developers.

**Why:** Parallel sessions work independently. Touching another session's dirty working tree causes conflicts and lost work.

**How to apply:** Before any work: check `git status`. If dirty tree with files you didn't touch → do not commit them. If you're on main with dirty tree from other sessions → do not touch it; ask the user or switch to your own feature branch.
