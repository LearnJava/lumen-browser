In progress: —

CSS rule: P1 does NOT implement CSS properties. P4 owns all CSS.
  P1 writes layout algorithms and box-tree structure only.
  When a new algorithm needs a CSS property → add // CSS: <prop> comment at
  the call site and add a line to STATUS-P4.md "Needs wiring".

Bug fixes rule: P1 does NOT fix bugs. Discovered bugs → add to BUGS.md + P5 picks up.

Next:
- accessibility-aria: ARIA role/state → accessibility tree (AXTree struct); Phase 2
- html5-insertion-modes-remaining: полный набор insertion modes (in-table, in-caption, in-cell, in-row, in-select, after-body, in-frameset, etc.) по HTML LS §13.2.6

Recent: svg-layout-basic (BoxKind::SvgRoot/SvgShape, SvgShapeKind rect/circle/ellipse/line/path, ViewBox scale+offset, collect_svg_shapes flat traversal, lay_out_svg_root replaced-element sizing, paint emit_svg_shape stub, 12 тестов, graphic test 47) 2026-05-27, colspan-rowspan (col_span/row_span на LayoutBox, span-aware column width + placement + rowspan height post-fix, 7 тестов) 2026-05-27, html-template-content (DocumentFragment + InTemplate mode + <template> парсинг content во fragment, 12 тестов) 2026-05-27, form-submit (Event::FormSubmit + find_ancestor_form + collect_dom_form_fields + build_form_submit + make_get_url + GET-навигация, 20 тестов) 2026-05-27, css-first-line-letter (PseudoKind::FirstLetter на первом тексте, is_first_line на lines[0], 3 новых теста) 2026-05-27, html-loading-lazy (loading="lazy" ImageRequest.is_lazy + JS _lumen_init_lazy_images/_lumen_deliver_lazy_images + shell proximity fetch, 9 тестов) 2026-05-26, html-full-tree-builder (HTML5 §13.2 insertion modes + adoption agency, 17 режимов, AAA, 349 тестов) 2026-05-26, phase0-close (Phase 0 закрыта, маркеры ✅ для html-parser/css-parser/layout) 2026-05-26, fix-inline-block-baseline (BUG-023 P1-часть — strut только для baseline-строк; TEST-12 PASS 0.18%, TEST-13 PASS 0.24%) 2026-05-26, fix-max-height (BUG-025 подтверждён в layout — release-бинарь был устаревшим, TEST-11 PASS 0.43%, unit tests для max-height/min-height/vertical-align:bottom добавлены) 2026-05-25, full HTML5 named entities WHATWG 2125 (gen_entities.py + бинпоиск + 338 тестов) 2026-05-25, push-tokenizer feed_bytes(&[u8]) с буферизацией partial UTF-8, 7 тестов (342 итого) 2026-05-25, ADR-инфраструктура docs/decisions/ (TEMPLATE.md + README + ADR-001..005) 2026-05-25
