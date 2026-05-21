In progress: —

Next (Wave 1 — unblock P1/P2):

Next (Wave 2 — unblock P2 animations):

Queue (Wave 3+):
CSS Containment enforcement (contain flags)                  ~2h
Container Queries @container matching                        ~3h  depends on containment
direction + bidi layout wire-up                              ~2h
mask-image/repeat/size → rendering                           ~2h

Filler (no dependencies, pick when idle):
hyphens engine + HyphenationProvider                         ~2h
tab-size rendering                                           ~1h
pointer-events + user-select wire-up                         ~1h
scroll-snap-* application                                    ~2h
text-emphasis rendering                                      ~1h
Update css-2026-compliance.md (Grid/Position/Transform outdated)

Coordination rules:
  — Before touching style.rs: check STATUS-P1.md, avoid same property area
  — Before touching display_list.rs / renderer.rs: notify P2 in commit message
  — Use separate worktree for every task: .claude/worktrees/<task>/
  — Merge to main after each property (keep divergence small)
  — Compliance tracker: css-2026-compliance.md

Recent: image-rendering bilinear/nearest GPU sampler ✅ 2026-05-21, multi-column lay_out_multicol_children column-count/width/gap ✅ 2026-05-21, background-repeat/position/size 🟡→✅ DrawBackgroundImage+renderer 2026-05-21, cursor OS wire-up HitTestResult.cursor+css_cursor_to_winit 2026-05-21, background-image gradients ParsedGradient+DrawLinearGradient 2026-05-21, transition wire-up TransitionScheduler 2026-05-21, animation wire-up @keyframes→AnimationScheduler 2026-05-21, vertical-align inline y-offset 2026-05-21, ::before/::after pseudo-element generation 2026-05-21, @font-face L4 descriptors 2026-05-21, fix-tests-garbled 2026-05-21, css-shapes + motion-path 2026-05-21, writing-mode 2026-05-21, backdrop-filter + print-color-adjust + font-size-adjust 2026-05-21, color-scheme 2026-05-21, text-underline-position 2026-05-21, orphans-widows 2026-05-21, line-clamp 2026-05-21, transform-matrix 2026-05-21, display-ext 2026-05-21, containment + container-queries 2026-05-21, text-align-last + touch-action + appearance 2026-05-21, forced-color-adjust 2026-05-21, resize + line-break 2026-05-21
