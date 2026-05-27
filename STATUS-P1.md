In progress: none (completed click-hint-overlay, merging)
Next step: (check lumen-plan.md Phase 2 — next P1 from Wave 2 Queue or Phase 2 system tasks)

CSS rule: P1 does NOT implement CSS properties. P4 owns all CSS.
  P1 writes layout algorithms and box-tree structure only.
  When a new algorithm needs a CSS property → add // CSS: <prop> comment at
  the call site and add a line to STATUS-P4.md "Needs wiring".

Bug fixes rule: P1 does NOT fix bugs. Discovered bugs → add to BUGS.md + P5 picks up.

Recent:
- click-hint-overlay (7B.2): enhance collect_clickable_elements with <details> support — add ClickableKind::Details variant, is_details_element() helper, comprehensive unit tests (6 new tests for link/button/input/details/mixed), P1 complete 2026-05-28 — P3 integration pending
- print-pdf-pagination (5++, Phase 1): PaginationContext + Page + PageFragment, paginate() algorithm for break-before/after/avoid, 7 unit tests, exports in lib.rs, clippy clean 2026-05-28
