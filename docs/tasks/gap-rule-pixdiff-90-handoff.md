# gap-rule-pixdiff-90 — handoff (WIP, slice 90 of CSS Gap Decorations L1)

Branch `p4-gap-rule-pixdiff-90`, slot `p4-work`. No code changed yet; analysis only.

## Baseline (`css-gaps`, 800×600, 313 pages, `.tmp/gp.py`, binary rebuilt from main 120accafd)

identical 296, thin-only 2, thick 15. Remaining: flex/fragmentation `008/013/020`, subgrid/fragmentation `011/013/020`,
`grid-gap-decorations-024/025/027` + `visited-alpha-clamp` (`:visited`, not worth it), `032/033`, multicol `006/007/writing-mode/multi-value-writing-mode`.

## Finding: `grid-gap-decorations-032/033` (and anything with `body { overflow: hidden }`)

The gap geometry is correct (`grid_gap_segments_logical` returns the 2 column + 5 row segments over the 350 px of overflowing
explicit tracks; checked with a debug dump). The rules are cut at the 50 px body box because paint clips to `body`'s padding box:
`overflow: hidden` on `body` (or `html`) is applied as an ordinary clip (`walk.rs` ~1214 `overflow_clips`, `box_layer.rs` ~124,
`scrollbars.rs` ~230, `text_run.rs` ~403). CSS Overflow L3 §3.5: the root's `overflow` (or `body`'s when the root's is `visible`)
goes to the viewport and the element's used value is `visible`. Minimal repro (Edge draws 20..120, Lumen stops at 70):

    html{overflow:hidden} body{margin:0} .a{height:50px} .b{height:100px;margin-top:20px;background:red}   /* <div class=a><div class=b> */

Without `overflow: hidden` on body, `.tmp/edge/o4.html` (static copy of 033 with rule lists) is pixel-identical to Edge.
Note 033 also sets `row-rule-width: repeat(4, 5px), 200px` from script (CSSOM) — check after the clip fix that the list is applied.

Where to fix: layout already knows the root/body pair (`collect_page_scroll_containers` in `lib.rs` ~1370: html = first non-Skip child
of the document box, body = first non-Skip child of html; body only counts when html's overflow is `visible`). Paint has no such
context (several entry points in `display_list/builder.rs`, `box_layer.rs`, `walk.rs`). Options: a `LayoutBox` flag set in
`box_tree/entry.rs` next to `propagate_viewport_scrollbar_gutter` (107 `LayoutBox {` literals — check for a constructor first), or a
helper `clips_overflow(b)` that paint sites call. Do NOT mutate `html.style.overflow` (the page-scroll filter reads it).
Then: `cargo clippy -p lumen-layout -p lumen-paint --all-targets -- -D warnings`, full `python graphic_tests/run.py --continue-on-fail`
(pixels move), tests for html/body propagation, CSS-SPECS.md row 113 + CAPABILITIES/doc-sync.

## Finding: multicol `writing-mode` pages (11400 / 21700 px) — not a quick win

`emit_column_rules` / `multicol_gap_decorations.rs` read geometry from child rects, but `lumen-layout` multicol is `HorizontalTb`-only
(`multicol_abspos.rs:27`), so there is nothing to map in vertical modes; Chrome draws column rules as horizontal bars between
stacked columns and the row rule as one vertical bar. Needs vertical multicol layout first.

## Tools in the slot (`.tmp/`, untracked)

`gp.py` (batch classify css-gaps), `dd.py <rel>` (diff one page, images in `.tmp/dd/`), `edge/cmp.py <name> <xs> <ymax>` (Edge vs Lumen of
`.tmp/edge/<name>.html`; its pixel count is wrong because Edge PNG has 3 bpp — trust the run lists), `.tmp/rusty_v8.lib` (build with
`RUSTY_V8_ARCHIVE=$(cygpath -w $PWD/.tmp/rusty_v8.lib)`; `target/dev-release/gn_out/obj/rusty_v8.lib` got truncated to 0 bytes once — re-copy from `.tmp`).
