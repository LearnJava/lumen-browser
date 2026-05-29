---
name: extras-p2 Phase 2 Print PDF rendering
description: Print PDF Phase 2 impl — display-list page breaks, renderer multi-page, shell --print-to-pdf
type: project
originSessionId: ee48823b-e223-4922-921b-7760a2e04ad6
---
**Phase 1 (завершена 2026-05-28):** `paginate()` алгоритм в `layout/src/pagination.rs`, Page/PageFragment структуры, break detection.

**Phase 2 (в работе, branch p1-extras-p2-phase2):**
1. DisplayCommand::PageBreak в paint/src/display_list.rs — маркер границы страницы для renderer
2. Renderer::render_pages(pages: Vec<Page>) → рисует каждую страницу отдельно в PDF context
3. Shell --print-to-pdf <output.pdf> → BrowserSession::print_to_pdf()
4. samples/heavy.html (150 MB target) для многостраничных бенчей

**Dependencies:**
- Нужен PDF-writer crate (provisional, see lumen-plan.md §5)
- Layout Phase 1 (paginate, break detection) ✅

**Next steps:**
1. paint/src/display_list.rs: PageBreak command enum variant
2. Renderer::render_pages() impl
3. Shell print-to-pdf CLI
4. Integration tests vs samples/heavy.html
