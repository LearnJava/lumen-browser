# Ph3 — Print pipeline (pagination + vector PDF + preview UI)

**Developer:** P1 + P2 + P4 · **Branch:** `p1-ph3-print-pipeline` · **Size:** L · **Crates:** `lumen-layout`, `lumen-paint`, `lumen-shell`, `lumen-css-parser` (P4)

---

## Status

**Phase 3 (v1.0) — FUTURE.** Roadmap item: `docs/plan/phases.md:133` —
"Print pipeline runtime `[P1+P2+P4]` — pagination algorithm over already-parsed
`@page` and break-* properties, PDF generation."

W-1 (Print PDF Phase 2, ✅ 2026-06-12) already shipped a working **raster-to-PDF**
pipeline end to end: layout → naive paginate → display list → render each page to an
`Image` → embed images as DeviceRGB XObjects in a PDF. There is also a print **dialog**
(E-1 / W-2b / CC-8). This task covers the three remaining pieces that turn that
prototype into a real print pipeline:

1. **Real pagination** — CSS Fragmentation L3 over `@page` + `break-*` (the current
   algorithm only splits top-level block children and never breaks *inside* a box).
2. **Vector / text PDF** — emit real text runs + vector graphics from the display list
   instead of one raster image per page.
3. **Print preview UI** — a visual page-by-page preview, not just the settings modal.

Do **not** start before Phase 3 opens (the team is still finishing Phase 2). This file
is the design spec so the work can begin cold.

---

## Goal

Produce print output that is **paginated by CSS rules** (not by clipping the screen
view), rendered as **selectable, vector PDF** (text as real glyphs, fills/borders as PDF
path operators), with a **live print preview** in the shell that reflects the chosen
`@page` size, margins, orientation, and scale.

---

## Current state (what exists today)

### Pagination (P1 domain) — `crates/engine/layout/src/pagination.rs`
- `PaginationContext` (page size + 4 margins), `Page`, `PageFragment`, `paginate()` —
  `pagination.rs:23`, `:67`, `:88`, `:112`.
- `paginate()` walks **only the direct block children of the root** and places each whole
  child on a page (`pagination.rs:120`). Limitations called out in its own doc comment
  (`pagination.rs:107-111`): **no break-inside, no nested splitting, no orphans/widows,
  no float/multicol awareness, single continuous reflow assumed.**
- `break-before` / `break-after` honoured only as `Always | Page` at the top level
  (`should_break_before`/`should_break_after`, `pagination.rs:230`, `:238`).
  `break-inside` is **never consulted**; `Avoid` is "simplified" / effectively ignored
  (`pagination.rs:148`, `should_avoid_break_*` are `#[allow(dead_code)]`,
  `pagination.rs:246`, `:254`).
- A box taller than the page is placed whole on a fresh page and overflows
  (`pagination.rs:168-186`) — no in-box fragmentation.

### `@page` + break-* parsing (P4 domain — mostly DONE)
- `break-before` / `break-after` / `break-inside` are parsed and stored:
  `crates/engine/layout/src/style.rs:12119-12129`, enum `BreakValue` at `style.rs:1397`,
  fields at `style.rs:2578`; whitelist in `crates/engine/css-parser/src/lib.rs:106-108`.
- `@page` at-rule is parsed: `crates/engine/css-parser/src/parser.rs:923`, `:2783`;
  `PageRule` is collected into the stylesheet (`crates/engine/layout/...:7304`
  `s.page_rules.extend(sheet.page_rules)`).
- `@page` matching + computed props live in `crates/engine/layout/src/page.rs`:
  `match_page_rules()` (`:544`, handles `:first/:last/:left/:right`),
  `compute_page_properties()` (`:614`), `PageProperties`, `PageBox`, margin-box layout
  (`page.rs:124`, `:356`, `layout_margin_boxes`). **GAP:** `extract_length_property`
  (`page.rs:595`) ignores units ("ignore units for now") and named `@page` selectors
  don't match yet (`page.rs:582-585`).
- **Wiring GAP (P4):** the print pipeline does **not** read parsed `@page` rules. Both
  `do_print_to_pdf` (`crates/shell/src/main.rs:1048`) and
  `do_print_to_pdf_with_opts` (`crates/shell/src/main.rs:1119`) build `PaginationContext`
  from **hardcoded / dialog** margins; the page-size + margins authored in `@page` CSS are
  never applied to the print job. `attach_page_boxes` (`main.rs:1154`) synthesises only a
  default "N / M" footer with a fixed 8px/char measurer.

### PDF generation (P2 domain) — raster only
- `Renderer::render_print_pages(font_bytes, pages, w, h) -> Vec<Image>`
  (`crates/engine/paint/src/renderer.rs:6737`): renders each page's display list to a CPU
  `Image` via the headless renderer.
- `encode_images_as_pdf(images, page_w, page_h) -> Vec<u8>`
  (`crates/shell/src/main.rs:1197`): one DeviceRGB XObject **raster image** per PDF page,
  uncompressed. **There is no text or vector content in the PDF — pages are pictures.**
- `build_print_display_list(pages)` (`crates/engine/paint/src/display_list.rs:1837`) and
  `split_at_page_breaks(cmds)` (`display_list.rs:1899`) already produce a per-page
  `Vec<DisplayCommand>`. `PageBreak` is a display command (`display_list.rs:791`), no-op
  on screen (`renderer.rs:5288`). The display list carries everything a vector PDF needs:
  `DrawText` (`display_list.rs:374`, has text + rect + font_size + color + variation axes),
  `FillRect` (`:330`), `FillRoundedRect` (`:337`), `DrawBorder` (`:343`),
  `DrawImage` (`:412`), gradients (`:473`–`:514`), clip/transform/opacity stack
  (`:514`–`:646`).

### Print preview / dialog UI (P4 domain) — settings modal exists, no visual preview
- `crates/shell/src/panels/print_panel.rs` — `Ctrl+P` modal (E-1 + W-2b + CC-8):
  paper size A4/Letter/Legal, orientation, margin presets, scale 50–200%, page range,
  color mode, "print backgrounds" toggle, output path, Print/Cancel
  (`print_panel.rs:1-12`, `PrintPanel` at `:111`, `PrintHit` at `:207`).
- Clicking **Print** calls `do_print_to_pdf_with_opts` (`main.rs:9080`, `:13066`).
  **GAP:** the panel shows *form controls only* — there is **no rendered preview** of the
  paginated pages, page count, or how `@page`/breaks affect the layout.
- CLI: `lumen --print-to-pdf <out.pdf> <path-or-url>` (`main.rs:771`, `:1018`;
  arg extraction `extract_print_to_pdf` `main.rs:1323`).

---

## Architecture

```
                 stylesheet @page rules + break-* (PARSED — P4 done)
                                  │
   ┌──────────────────────────────┼───────────────────────────────────────┐
   │ P1: real pagination          │                                        │
   │   LayoutBox tree ──► fragment recursively over @page page-box +        │
   │   break-before/after/inside ──► Vec<Page> of positioned fragments      │
   │   (orphans/widows, in-box splitting, taller-than-page handling)        │
   └──────────────────────────────┬───────────────────────────────────────┘
                                   │ Vec<Page> + per-page PageBox
                build_print_display_list ──► Vec<Vec<DisplayCommand>>
                                   │
   ┌───────────────────────────────┼──────────────────────────────────────┐
   │ P2: vector / text PDF          │                                       │
   │   per-page DisplayCommand list ──► PDF content stream:                 │
   │     DrawText  ──► PDF text-showing ops (embedded subset font)          │
   │     Fill*/Border/gradient ──► PDF path/shading operators               │
   │     DrawImage ──► XObject (raster only where unavoidable)              │
   └───────────────────────────────┬──────────────────────────────────────┘
                                    │ Vec<u8> PDF
   ┌────────────────────────────────┼─────────────────────────────────────┐
   │ P4: print preview UI                                                   │
   │   print_panel ──► render paginated pages as thumbnails / page strip,   │
   │   live page count, reflect @page size+margins+orientation+scale,       │
   │   wire parsed @page into PaginationContext (the missing wiring)        │
   └───────────────────────────────────────────────────────────────────────┘
```

- **P1 (pagination/fragmentation over `@page` + break-* → page boxes).** Rewrite
  `paginate()` from a top-level-children loop into a recursive fragmentation pass that:
  descends into nested block boxes, honours `break-inside: avoid` (keep a subtree on one
  page), splits a box across pages when it is taller than the content box, and applies
  orphans/widows on line boxes. Takes the per-page `PageProperties` (size + margins) from
  `@page` as input rather than a single global `PaginationContext`.
- **P2 (vector / text PDF from display list).** New PDF encoder that consumes per-page
  `Vec<DisplayCommand>` and writes a real PDF content stream: text via embedded
  (subset) font + text-showing operators so text is selectable/searchable; `FillRect`,
  `FillRoundedRect`, `DrawBorder`, gradients via PDF path/shading operators; `DrawImage`
  stays a raster XObject. Keep `encode_images_as_pdf` as a raster fallback.
- **P4 (print preview UI + `@page` options).** Wire parsed `@page` rules into the print
  job (the missing link: read `page_rules` → `match_page_rules`/`compute_page_properties`
  → per-page size+margins instead of hardcoded values), finish unit handling in
  `extract_length_property`, and extend `print_panel` with a visual page-strip preview
  (thumbnails of the paginated output, live page count).

---

## Team split

| Dev | Owns | Deliverable |
|---|---|---|
| **P1** | `lumen-layout` (`pagination.rs`) | Recursive fragmentation: nested split, `break-inside`, orphans/widows, taller-than-page split; per-page `PageProperties` input. |
| **P2** | `lumen-paint` + new encoder | Vector/text PDF from per-page `Vec<DisplayCommand>` (selectable text + vector fills). Raster path kept as fallback. |
| **P4** | `lumen-css-parser`, `lumen-layout` (`page.rs`), `lumen-shell` (`print_panel.rs`) | Wire parsed `@page` into the pipeline; finish length-unit parsing + named-page matching; visual print-preview page strip. |

Hand-offs (interface-first):
- P1 publishes the new `paginate(...) -> Vec<Page>` signature (taking `&[PageProperties]`
  or a `&dyn Fn(u32) -> PageProperties`) with a `todo!()` stub first; P2 and P4 build
  against it.
- P2 publishes `encode_pages_as_pdf(pages: &[Vec<DisplayCommand>], &[PageProperties])`
  signature stub; P4's preview and the shell call sites migrate to it.
- P4 marks the `@page`-wiring point with `// CSS: @page` at `main.rs:1048` / `:1119`.

---

## Entry points (real file:line; *(proposed)* = to be created)

**P1 — pagination**
- `crates/engine/layout/src/pagination.rs:112` — `paginate()` — rewrite to recursive
  fragmentation.
- `crates/engine/layout/src/pagination.rs:230`–`:255` — `should_break_*` /
  `should_avoid_break_*` — promote `Avoid`/`break-inside` from dead code to live logic.
- `crates/engine/layout/src/pagination.rs:67`–`:95` — `Page` / `PageFragment` — may need a
  `fragment_clip` (which slice of a split box this fragment shows) *(proposed field)*.
- *(proposed)* `fn fragment_box(box, available_height) -> (PageFragment, Option<LayoutBox>)`
  in `pagination.rs` — split one box, return the remainder for the next page.

**P2 — vector PDF**
- `crates/shell/src/main.rs:1197` — `encode_images_as_pdf` — keep as raster fallback.
- *(proposed)* `crates/engine/paint/src/pdf.rs` (or `crates/shell/src/print/pdf.rs`) —
  `encode_pages_as_pdf(pages: &[Vec<DisplayCommand>], props: &[PageProperties]) -> Vec<u8>`
  vector encoder.
- `crates/engine/paint/src/display_list.rs:374` (`DrawText`), `:330`/`:337`/`:343`
  (`FillRect`/`FillRoundedRect`/`DrawBorder`), `:473`–`:514` (gradients), `:412`
  (`DrawImage`) — the commands the encoder must translate.
- `crates/engine/paint/src/renderer.rs:6737` — `render_print_pages` — reused only for the
  raster fallback path.

**P4 — `@page` wiring + preview**
- `crates/shell/src/main.rs:1048` and `:1119` — build `PaginationContext` / call sites —
  add `// CSS: @page` and source size+margins from parsed `@page`.
- `crates/engine/layout/src/page.rs:544` (`match_page_rules`), `:614`
  (`compute_page_properties`), `:595` (`extract_length_property` — finish units), `:582`
  (named-page matching) — the `@page` resolution already present, to be invoked + finished.
- `crates/shell/src/panels/print_panel.rs:111` (`PrintPanel`), `:207` (`PrintHit`) — extend
  for a preview strip + live page count.
- *(proposed)* `crates/shell/src/panels/print_preview.rs` — render paginated pages as
  thumbnails for the modal.

---

## Steps

**P1**
1. Change `paginate()` to take per-page page geometry (from `@page`) instead of one global
   `PaginationContext`; keep a back-compat overload for the CLI default.
2. Make the walk **recursive**: descend into block subtrees, accumulating y within the
   content box of the current page.
3. Honour `break-inside: avoid` — try to keep a subtree whole; push to next page if it
   fits there, else allow the split.
4. Implement **in-box splitting**: when a block is taller than remaining (or full) content
   height, split at a line-box / child boundary; carry the remainder to the next page.
5. Apply **orphans / widows** on the line boxes at split points.
6. Unit tests for each rule (see Tests).

**P2**
7. Add the vector PDF encoder skeleton consuming `&[Vec<DisplayCommand>]`; embed a subset
   of the Inter font; map `DrawText` → text-showing ops in page coordinates.
8. Map `FillRect`/`FillRoundedRect`/`DrawBorder`/gradients → PDF path + shading operators;
   `DrawImage` → XObject. Honour the clip/opacity/transform stack as graphics-state ops
   (approximate where PDF can't express it).
9. Swap the shell call sites to the vector encoder; keep `encode_images_as_pdf` reachable
   as a `--raster` fallback.

**P4**
10. Read `page_rules` from the parsed document in `do_print_to_pdf*`; resolve per-page
    `PageProperties` via `match_page_rules` + `compute_page_properties`; feed P1's
    `paginate()`. Finish `extract_length_property` units (cm/mm/in/px → px) and named-page
    matching.
11. Make the dialog's paper-size/orientation/margins/scale interact correctly with
    authored `@page` (CSS `@page` as the base, dialog as override or vice-versa — document
    the precedence chosen).
12. Add the preview page strip: render the paginated `Vec<Page>` to thumbnails, show live
    page count, update on every settings change.

---

## Tests

**P1 (`cargo test -p lumen-layout`)** — extend `pagination.rs` tests (`pagination.rs:258`):
- `break-before: page` / `break-after: page` force a new page (already covered for
  top-level; add **nested** cases).
- `break-inside: avoid` keeps a subtree on one page when it fits on the next.
- A block taller than the content box splits across ≥2 pages (in-box fragmentation).
- Orphans/widows: a 1-line tail is pulled to the next page when widows ≥ 2.
- Empty document → exactly one (empty) page.

**P2 (`cargo test -p lumen-paint` / `lumen-shell`)**:
- Vector PDF starts with `%PDF-`, contains a Font object and a text-showing operator for a
  known string (extend the existing `encode_images_as_pdf_*` tests at `main.rs:16493`).
- A page with a `FillRect` emits a path-fill operator (not an image XObject).
- Page count in the PDF equals `split_at_page_breaks(...).len()`.

**P4 (`cargo test -p lumen-shell` / `lumen-layout`)**:
- `@page { size: A4; margin: 2cm }` produces the matching `PageProperties` (units parsed) —
  extend `page.rs` tests.
- `@page :first { margin-top: 4cm }` applies only to page 0 in the print job.
- Print job margins come from `@page` when present, dialog otherwise (precedence test).

**Graphic / golden:** add a multi-page sample to `graphic_tests/` (or a dedicated print
fixture) exercising `break-*` + `@page`, and assert the PDF page count + that text is
present as a string in the PDF bytes (selectability proxy). No threshold change.

---

## Definition of done

- [ ] `paginate()` fragments **recursively**, honours `break-before/after/inside` and
      orphans/widows, and splits boxes taller than the page across pages (P1).
- [ ] Print PDFs contain **selectable text** and **vector** fills/borders, not just raster
      images; `encode_images_as_pdf` remains as an explicit raster fallback (P2).
- [ ] Authored `@page` size + margins (+ `:first/:left/:right`) drive the print job, with
      units parsed; dialog precedence documented (P4).
- [ ] The print modal shows a **visual page-strip preview** with live page count (P4).
- [ ] `lumen --print-to-pdf` and the `Ctrl+P` dialog both produce identical paginated,
      vector output.
- [ ] `cargo clippy -p <crate> --all-targets -- -D warnings` clean for each touched crate;
      `cargo test -p lumen-layout`, `-p lumen-paint`, `-p lumen-shell` pass.
- [ ] Docs updated in the same commit: `CAPABILITIES.md` (print line `:88`),
      `subsystems/layout.md` + `subsystems/paint.md`, `STATUS-P1/P2/P4.md`, `SYMBOLS.md`
      (new `pub fn`s), `CSS-SPECS.md` for `@page`/`break-*` rows.

---

## Notes

- W-1 already delivered raster-to-PDF — **do not re-implement it**; this task is the
  delta: real fragmentation, vector PDF, preview.
- `@page` and `break-*` are **already parsed** (P4) — the open P4 work is *wiring* them
  into the print job + finishing unit/named-selector handling, not parsing from scratch.
- Keep the headless `render_print_pages` path working; the vector encoder is additive.
