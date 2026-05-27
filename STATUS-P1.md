In progress: form-submit  branch: p1-form-submit
Next step: add FormSubmit event variant  crates/core/src/event.rs

CSS rule: P1 does NOT implement CSS properties. P4 owns all CSS.
  P1 writes layout algorithms and box-tree structure only.
  When a new algorithm needs a CSS property → add // CSS: <prop> comment at
  the call site and add a line to STATUS-P4.md "Needs wiring".

Bug fixes rule: P1 does NOT fix bugs. Discovered bugs → add to BUGS.md + P5 picks up.

Next:
- form-submit: полный алгоритм form submission (action= + method=GET/POST + URLSearchParams encoding) → Event «FormSubmit» в EventSink
- html-template-content: `<template>` element — парсить content во fragment (DocumentFragment, inert subtree), attach при clone; нужно для Web Components
- colspan-rowspan: table layout colspan/rowspan атрибуты — compute_table_col_widths учитывает span

Queue (Wave 3+):
- svg-layout-basic: базовый SVG layout pass (viewBox + basic shapes: rect/circle/line/path как CSS-боксы); Phase 2
- accessibility-aria: ARIA role/state → accessibility tree (AXTree struct); Phase 2
- html5-insertion-modes-remaining: полный набор insertion modes (in-table, in-caption, in-cell, in-row, in-select, after-body, in-frameset, etc.) по HTML LS §13.2.6

Recent: css-first-line-letter (PseudoKind::FirstLetter на первом тексте, is_first_line на lines[0], 3 новых теста) 2026-05-27, html-loading-lazy (loading="lazy" ImageRequest.is_lazy + JS _lumen_init_lazy_images/_lumen_deliver_lazy_images + shell proximity fetch, 9 тестов) 2026-05-26, html-full-tree-builder (HTML5 §13.2 insertion modes + adoption agency, 17 режимов, AAA, 349 тестов) 2026-05-26, phase0-close (Phase 0 закрыта, маркеры ✅ для html-parser/css-parser/layout) 2026-05-26, fix-inline-block-baseline (BUG-023 P1-часть — strut только для baseline-строк; TEST-12 PASS 0.18%, TEST-13 PASS 0.24%) 2026-05-26, fix-max-height (BUG-025 подтверждён в layout — release-бинарь был устаревшим, TEST-11 PASS 0.43%, unit tests для max-height/min-height/vertical-align:bottom добавлены) 2026-05-25, full HTML5 named entities WHATWG 2125 (gen_entities.py + бинпоиск + 338 тестов) 2026-05-25, push-tokenizer feed_bytes(&[u8]) с буферизацией partial UTF-8, 7 тестов (342 итого) 2026-05-25, ADR-инфраструктура docs/decisions/ (TEMPLATE.md + README + ADR-001..005) 2026-05-25
