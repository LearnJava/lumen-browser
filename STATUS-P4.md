In progress: border-radius elliptical (rx≠ry)  branch: p4-border-radius-elliptical
Next step: commit + merge  crates/engine/layout/src/style.rs, crates/engine/paint/src/display_list.rs, crates/engine/paint/src/renderer.rs

Next:

Queue (Wave 3+):

Role: P4 owns ALL CSS work. P1/P2/P3 do not write CSS properties.
Full property roadmap and work queue — CSS-SPECS.md (P4 Priority Queue section).

Needs wiring (algorithm ready, CSS not connected):
  — :host / ::slotted pseudo-classes (Shadow DOM cascade + composed tree merged).
    P4 task: implement :host selector matching in `matches_complex` (matches shadow
    host from inside shadow tree) + ::slotted() functional pseudo-element matching.
    Wire point: build_box() in layout/src/box_tree.rs — see comment
    "// CSS: :host, ::slotted — P4 wires shadow-scoped styles here".
    Branch: new branch for this P4 task.

  — @font-face registry wired in shell (branch: font-face-loading).
    P4 task: wire font-weight/font-style/font-stretch descriptors from
    FontFaceRule into FontRegistry::register_from_bytes calls.
    CSS: @font-face unicode-range, font-display — P4 deferred.
  — CSS Transitions sync(): TransitionScheduler::tick() is wired into the
    shell frame loop (P2, branch animation-transition-engine). P4 task: call
    transition_scheduler.sync(node, &old_style, &new_style, now) after every
    relayout or computed-style mutation so transitions actually fire.
    Location: shell/src/main.rs — relayout() and apply_loaded_page().
    CSS: transition-property / transition-duration / transition-delay /
    transition-timing-function already in ComputedStyle (P4 2026-05-21).

Coordination rules:
  — Before touching style.rs: check STATUS-P1.md, avoid same property area
  — Before touching display_list.rs / renderer.rs: notify P2 in commit message
  — Use separate worktree for every task: .claude/worktrees/<task>/
  — Merge to main after each property (keep divergence small)
  — Compliance tracker: CSS-SPECS.md (единственный источник правды)
  — P1/P2/P3 handoff: when they add a new algorithm stub marked // CSS: <prop>,
    it appears in "Needs wiring" above — pick it up as a P4 task

Recent: CSS Positioning L3 §6.3 position:sticky BeginStickyLayer/EndStickyLayer+sticky_offset_dy/dx+to_px_opt()+5 тестов+graphic test 42 CSS-SPECS.md #6 🟡→✅ 2026-05-24, CSS Display L3 §17 table layout BoxKind::Table+TableRowGroup+lay_out_table+compute_table_col_widths global col widths+6 тестов+graphic test 41 CSS-SPECS.md #5 🟡→✅ 2026-05-24, CSS Images L4 §3.7 conic-gradient() ParsedGradient::Conic + parse_conic_gradient_params + parse_conic_stop_position (deg/rad/turn/grad→%) + DrawConicGradient + PushMaskConicGradient + GradParamsCpu.param0 + WGSL kind=2 (box-space atan2 + repeating tile-by-stop-span) + 9 unit tests (layout) + 3 display-list tests + graphic test 40 + CSS-SPECS.md #17 ⬜→✅ 2026-05-24, CSS Easing L2 linear() easing function TimingFunction::LinearStops+LinearEasingPoint+parse_linear_easing_stops+linear_stops_progress+13 тестов CSS-SPECS.md #48 ⬜→✅ 2026-05-24, z-index stacking context paint ordering StackingTree+PaintOrder+build_display_list_ordered wired in shell + build_display_list_ordered_with_anim + graphic test 38 ✅ 2026-05-23, float+clear CSS 2.1 §9.5 FloatContext+FloatSide+ClearSide+10 тестов ✅ 2026-05-22, CSS Grid L1 grid-template-areas + GridLine::Named + find_named_area + resolve_named_lines + 9 тестов ✅ 2026-05-22, CSS Nesting L1 implicit nesting + nested at-rules + NestedRule::Qualified/AtRule + 20 тестов ✅ 2026-05-22, @layer cascade ordering CSS Cascade L5 §6.4.5 layer_priority sort key + 6 tests ✅ 2026-05-22, CSS Transitions wire-up TransitionScheduler sync()+tick() in shell + CSS-SPECS.md #1/#2/#3 → ✅ 2026-05-22, CSS-SPECS.md sync filter/clip-path/column-rule/text-emphasis/scroll-snap/container ✅ 2026-05-22, tab-size rendering white-space:pre/pre-wrap + \t→tab_size + UA <pre> ✅ 2026-05-22, css-2026-compliance.md Grid/Position/Transform/hyphens/cursor актуализирован ✅ 2026-05-22, hyphens Manual/Auto wrap_inline_run+HyphenationProvider+layout_measured_hyp ✅ 2026-05-22, scroll-snap-* find_scroll_snap_y+proximity ✅ 2026-05-22, text-emphasis rendering emit_text_emphasis_marks per-char marks ✅ 2026-05-22, pointer-events+user-select wire-up UserSelect в HitTestResult ✅ 2026-05-22, @container queries matching ContainerContext+evaluate_container_condition+apply_container_styles 🟡 2026-05-22, CSS Containment enforcement contain:size/layout/paint 🟡+ 2026-05-22, direction+bidi layout wire-up TextAlign::Start/End+RTL mirroring 🟡+ 2026-05-21, mask-image/repeat/size PushMask*/PopMask+GPU mask_composite_pipeline 🟡 2026-05-21, image-rendering bilinear/nearest GPU sampler ✅ 2026-05-21, multi-column lay_out_multicol_children column-count/width/gap ✅ 2026-05-21, background-repeat/position/size 🟡→✅ DrawBackgroundImage+renderer 2026-05-21, cursor OS wire-up HitTestResult.cursor+css_cursor_to_winit 2026-05-21, background-image gradients ParsedGradient+DrawLinearGradient 2026-05-21, transition wire-up TransitionScheduler 2026-05-21, animation wire-up @keyframes→AnimationScheduler 2026-05-21, vertical-align inline y-offset 2026-05-21, ::before/::after pseudo-element generation 2026-05-21, @font-face L4 descriptors 2026-05-21, fix-tests-garbled 2026-05-21, css-shapes + motion-path 2026-05-21, writing-mode 2026-05-21, backdrop-filter + print-color-adjust + font-size-adjust 2026-05-21, color-scheme 2026-05-21, text-underline-position 2026-05-21, orphans-widows 2026-05-21, line-clamp 2026-05-21, transform-matrix 2026-05-21, display-ext 2026-05-21, containment + container-queries 2026-05-21, text-align-last + touch-action + appearance 2026-05-21, forced-color-adjust 2026-05-21, resize + line-break 2026-05-21
