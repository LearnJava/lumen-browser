In progress: text-decoration-thickness 🟡→✅  branch: text-decoration-thickness
Next step: update css-2026-compliance.md + add paint test

Next: position: relative offsets 🟡→✅   css-2026-compliance.md (already impl'd)

Queue (🟡→✅, paint/layout wire-up, no new parsing needed):
  text-overflow: ellipsis              display_list.rs — clip text run, emit "…"
  text-decoration-thickness            display_list.rs — pass resolved px to DrawText
  position: relative offsets           box_tree.rs — apply top/left/right/bottom after layout
  background-image url()               display_list.rs + renderer.rs (coordinate P2)
  transform matrix                     display_list.rs — emit DrawTransform (coordinate P2)

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

Recent: —
