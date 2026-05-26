In progress: —

CSS rule: P1 does NOT implement CSS properties. P4 owns all CSS.
  P1 writes layout algorithms and box-tree structure only.
  When a new algorithm needs a CSS property → add // CSS: <prop> comment at
  the call site and add a line to STATUS-P4.md "Needs wiring".

Next:
- phase0-close: lumen-plan.md — обновить маркеры 🟡→✅ для P1-крейтов (html-parser, css-parser, layout), объявить Phase 0 закрытой
- fix-list-markers-test32: маркеры отображаются в тесте не там (TEST-32 8.61%) — layout/src/box_tree.rs BoxKind::Marker placement
- fix-direction-rtl-alignment: bidi text alignment (TEST-27 9.35%) — P1-часть: layout TextAlign::Start/End mirror logic
- html-full-tree-builder: HTML5 §13.2 proper insertion modes — adoption agency algorithm, table/list/formatting element reconstruction; текущий tree builder lenient, не обрабатывает mismatched tags корректно

Queue (Wave 2):
- fix-border-style-dashed: TEST-21 border-style дострались dotted/dashed патчами; реализовать Bresenham dash-pattern в layout stroke helper (// CSS: border-style — P4 wires)
- css-has-selector: `:has(S)` matching algorithm в layout/src/selector.rs — парсер готов (css-parser), matching не реализован
- html-loading-lazy: `loading="lazy"` — P1-часть: emit IntersectionObserver event source при добавлении <img loading=lazy> в flat tree; координация с P3 (JS-сторона готова)
- css-first-line-letter: `::first-line` / `::first-letter` split в collect_inline_segments (box_tree.rs); expose pseudo kind → // CSS: ::first-line, ::first-letter — P4 wires styles
- form-submit: полный алгоритм form submission (action= + method=GET/POST + URLSearchParams encoding) → Event «FormSubmit» в EventSink
- html-template-content: `<template>` element — парсить content во fragment (DocumentFragment, inert subtree), attach при clone; нужно для Web Components
- colspan-rowspan: table layout colspan/rowspan attrибуты — compute_table_col_widths учитывает span

Queue (Wave 3+):
- svg-layout-basic: базовый SVG layout pass (viewBox + basic shapes: rect/circle/line/path как CSS-боксы); Phase 2
- accessibility-aria: ARIA role/state → accessibility tree (AXTree struct); Phase 2
- html5-insertion-modes-remaining: полный набор insertion modes (in-table, in-caption, in-cell, in-row, in-select, after-body, in-frameset, etc.) по HTML LS §13.2.6

Recent: fix-inline-block-baseline (BUG-023 P1-часть — strut только для baseline-строк; TEST-12 PASS 0.18%, TEST-13 PASS 0.24%) 2026-05-26, fix-max-height (BUG-025 подтверждён в layout — release-бинарь был устаревшим, TEST-11 PASS 0.43%, unit tests для max-height/min-height/vertical-align:bottom добавлены) 2026-05-25, full HTML5 named entities WHATWG 2125 (gen_entities.py + бинпоиск + 338 тестов) 2026-05-25, push-tokenizer feed_bytes(&[u8]) с буферизацией partial UTF-8, 7 тестов (342 итого) 2026-05-25, ADR-инфраструктура docs/decisions/ (TEMPLATE.md + README + ADR-001..005) 2026-05-25, CSS Counters resolution CSS Lists L3 §6.4 (counter-reset/increment, counter()/counters()/attr() в content:, CounterMap pre-pass, format_counter decimal/alpha/roman, 10 тестов) 2026-05-25, cq* units CSS Container Queries L1 §6.2 (cqw/cqh/cqi/cqb/cqmin/cqmax, thread-local CONTAINER_CQ, 4 тесты) 2026-05-25, CSS 2.1 §10.8.1 half-leading в inline box (apply_inline_vertical_align: y_offset=half_leading для baseline, ascent_px() в TextMeasurer, 4 тесты) + shell compile fix BeginStickyLayer/EndStickyLayer 2026-05-24, CSS 2.1 §10.3.3 margin:auto horizontal centering (both-auto=center, left-auto=flush-right) + 6 тестов 2026-05-24, CSS Grid §8.5 dense auto-placement (row dense / column dense) + span-on-start bug fix + 3 тесты 2026-05-24, Quirks mode §3.5 html height:100vh (viewport basis для body height:100%) + 5 тестов 2026-05-24, text-wrap-mode: nowrap + overflow-wrap: break-word + word-break: break-all/break-word wired to wrap_inline_run, char_break_offset() helper, 6 тестов 2026-05-24, margin collapsing CSS 2.1 §8.3.1 FIXED (prev_block_mb в child-loop box_tree.rs + 3 тест) 2026-05-24, BUG-020 per-axis overflow clip FIXED (box_layer_ops BIG-сентинели) + 3 regression tests 2026-05-24, BUG-023 P1-часть FIXED strut в InlineBlockRow без текста (Edge не добавляет font-strut в строках только из inline-block) + 3 regression tests 2026-05-24, BUG-004 FIXED height on inline elements (display:inline-block applies, display:inline ignores per CSS 2.1 §10.6.1) + 3 regression tests 2026-05-24, ::before/::after inline (collect_inline_segments) 2026-05-22, BUG-034 FIXED transform-origin 50% 50% не резолвился → rotate/scale вращались вокруг (0,0) 2026-05-22, Shadow DOM composed tree (FlatTree) + layout wiring 2026-05-22, display:flow-root BFC + display:contents elimination 2026-05-22, BUG-011 FIXED (::marker box + list-item layout) 2026-05-22, BUG-026 FIXED (img renders at correct CSS size) 2026-05-22, BUG-025 FIXED (InlineSpace shrink-to-fit) 2026-05-22, Forms ValidityState :valid/:invalid 2026-05-22, BUG-013 display:none breaks inline context 2026-05-22, bug-024-box-sizing 2026-05-21
