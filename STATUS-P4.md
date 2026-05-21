In progress: transform matrix  branch: transform-matrix
Next step: create 22-transform.html graphic test + update compliance tracker

Next: line-clamp                           style.rs — -webkit-line-clamp compat

Queue (⬜→🟡, new parse+store only, no paint):
  line-clamp                           style.rs — -webkit-line-clamp compat
  orphans / widows                     style.rs — fragmentation hints
  text-underline-position              style.rs — CSS Text Decoration L3
  color-scheme                         style.rs — CSS Color Adjustment L1

Coordination rules:
  — Before touching style.rs: check STATUS-P1.md, avoid same property area
  — Before touching display_list.rs / renderer.rs: notify P2 in commit message
  — Use separate worktree for every task: .claude/worktrees/<task>/
  — Merge to main after each property (keep divergence small)
  — Spec links: https://www.w3.org/TR/css-align-3/ etc.
  — Compliance tracker: css-2026-compliance.md

Recent: background-image-url 2026-05-20 (DrawBackgroundImage paint pipeline + collect_background_image_requests + shell fetch, 12 tests), text-overflow 2026-05-20, text-decoration-thickness 2026-05-20
