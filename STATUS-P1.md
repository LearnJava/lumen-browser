In progress: —

CSS rule: P1 does NOT implement CSS properties. P4 owns all CSS.
  P1 writes layout algorithms and box-tree structure only.
  When a new algorithm needs a CSS property → add // CSS: <prop> comment at
  the call site and add a line to STATUS-P4.md "Needs wiring".

Bug fixes rule: P1 does NOT fix bugs. Discovered bugs → add to BUGS.md + P5 picks up.

Next:
- html-full-tree-builder: HTML5 §13.2 proper insertion modes — adoption agency algorithm, table/list/formatting element reconstruction; текущий tree builder lenient, не обрабатывает mismatched tags корректно
- css-has-selector: `:has(S)` matching algorithm в layout/src/selector.rs — парсер готов (css-parser), matching не реализован
- html-loading-lazy: `loading="lazy"` — P1-часть: emit IntersectionObserver event source при добавлении <img loading=lazy> в flat tree; координация с P3 (JS-сторона готова)
- css-first-line-letter: `::first-line` / `::first-letter` split в collect_inline_segments (box_tree.rs); expose pseudo kind → // CSS: ::first-line, ::first-letter — P4 wires styles
- form-submit: полный алгоритм form submission (action= + method=GET/POST + URLSearchParams encoding) → Event «FormSubmit» в EventSink
- html-template-content: `<template>` element — парсить content во fragment (DocumentFragment, inert subtree), attach при clone; нужно для Web Components
- colspan-rowspan: table layout colspan/rowspan атрибуты — compute_table_col_widths учитывает span

Queue (Wave 3+):
- svg-layout-basic: базовый SVG layout pass (viewBox + basic shapes: rect/circle/line/path как CSS-боксы); Phase 2
- accessibility-aria: ARIA role/state → accessibility tree (AXTree struct); Phase 2
- html5-insertion-modes-remaining: полный набор insertion modes (in-table, in-caption, in-cell, in-row, in-select, after-body, in-frameset, etc.) по HTML LS §13.2.6

Recent: phase0-close (Phase 0 закрыта, маркеры ✅ для html-parser/css-parser/layout) 2026-05-26, fix-inline-block-baseline (BUG-023 P1-часть — strut только для baseline-строк; TEST-12 PASS 0.18%, TEST-13 PASS 0.24%) 2026-05-26, fix-max-height (BUG-025 подтверждён в layout — release-бинарь был устаревшим, TEST-11 PASS 0.43%, unit tests для max-height/min-height/vertical-align:bottom добавлены) 2026-05-25, full HTML5 named entities WHATWG 2125 (gen_entities.py + бинпоиск + 338 тестов) 2026-05-25, push-tokenizer feed_bytes(&[u8]) с буферизацией partial UTF-8, 7 тестов (342 итого) 2026-05-25, ADR-инфраструктура docs/decisions/ (TEMPLATE.md + README + ADR-001..005) 2026-05-25, CSS Counters resolution CSS Lists L3 §6.4 (counter-reset/increment, counter()/counters()/attr() в content:, CounterMap pre-pass, format_counter decimal/alpha/roman, 10 тестов) 2026-05-25, cq* units CSS Container Queries L1 §6.2 (cqw/cqh/cqi/cqb/cqmin/cqmax, thread-local CONTAINER_CQ, 4 тесты) 2026-05-25, CSS 2.1 §10.8.1 half-leading + margin:auto + Grid dense + Quirks 100vh + text-wrap-mode + margin collapsing + BUG-023/020/004 2026-05-24, Shadow DOM FlatTree + layout wiring + display:flow-root + ::before/::after + BUG-034/011/026/025/013 2026-05-22, bug-024-box-sizing 2026-05-21
