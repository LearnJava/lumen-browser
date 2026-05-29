---
name: Merge workflow — always pull main first
description: Before merging feature branch to main, merge main into the branch and resolve conflicts locally
type: feedback
originSessionId: 57e8594b-433a-47dd-a245-9d53c650d71f
---
Before final merge to main: `git merge main` → resolve conflicts in feature branch → then merge back to main.

**Why:** Prevents conflicts from appearing in main. Cleaner merge history, no "Merge main into feature-X" commits polluting main.

**How to apply:** In feature branch: `git merge main` (conflicts resolved here if any) → `git checkout main && git merge --no-ff <feature-branch>` (clean fast-forward or simple merge).
