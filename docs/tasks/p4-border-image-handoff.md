# p4-border-image — handoff (WIP, branch `p4-border-image`)

Task: `border-image` / `border-image-{source,slice,width,outset,repeat}` (CSS-SPECS.md row, BUG-492). Work in the main checkout (use_worktree off).

## Done (layout side, `cargo check -p lumen-layout` passes; unit tests not run yet)

- `crates/engine/layout/src/style/values/border_image.rs` — types `BorderImage{source,source_css,slice,fill,width,outset,repeat}`, parsers for the five longhands + shorthand, computed serialisation (`longhand_css`, `shorthand_css`), `canonical_specified_border_image` (inline `el.style`), unit tests (not yet run).
- Field `ComputedStyle::border_image` (+ both constructors), `apply/paint.rs` branches (`border` shorthand resets it), `apply/css_wide.rs` (inherit/initial), `selector_query.rs` computed map, `SUPPORTED_PROPERTIES`, `image_requests.rs` (source URL is fetched), `background_cssom.rs` + `web_api_shim_mid.js` (CSSOM canonical).

## Next (paint side — design decided)

1. Display list has no image sizes (renderer knows them), so add `DisplayCommand::DrawBorderImage { area: Rect /*border box + outset*/, border_widths:[f32;4], src:String, spec: BorderImageSpec }` in `paint/src/display_list/commands.rs` (+ `kind_name`, `bounds`, `translate.rs`, `fingerprint.rs` (x2), `serialize.rs`, the list of names ~line 1088). `BorderImageSpec` (serde) lives in a new `paint/src/border_image.rs`: slice `[Num|Pct;4]`, `fill`, width `[Px|Auto;4]` (Number → n*border width, Percent → pct*area dim, resolved at emit), repeat `[h,v]`.
2. Shared pure fn `border_image_quads(spec, area, widths, img_w, img_h) -> Vec<{src: Rect(image px), dst: Rect}>` implementing CSS Backgrounds 3 §6.2–6.5: slice clamp, 9 regions, width scaling when widths exceed area (reduce factor f), stretch / repeat / round / space for the 4 edges and the middle (with `fill`), cap the tile count (~1024/edge).
3. Backends: cpu_raster — `blit_image_into_rect(pm, placed, visible, img, 1.0, clip)` per quad with `placed` = full image stretched so slice maps onto dst (`placed = (dst.x - src.x*sx, dst.y - src.y*sy, W*sx, H*sy)`, `visible = dst`); wgpu `renderer.rs` — `push_image_quad(&mut image_vertices, dst, [u0,v0],[u1,v1], 1.0)` with UV = src/size (see `DrawBackgroundImage`, ~line 2044); femtovg fallback — at least compile (no-op or same idea).
4. Emit (`display_list/walk.rs` block path ~line 62, `has_border` check): if `s.border_image.is_active()` → emit `DrawBorderImage` instead of `emit_box_border` (URL; `image-set()` via `select_image_set_url`). Gradient source: lower at emit time with the same quads (image size = area): per quad `PushClipRect(dst)` + `Draw*Gradient` on rect mapped from the full image rect. CrossFade source → draw nothing.
5. Tests: layout unit tests (`cargo test -p lumen-layout border_image`), quads fn tests, display-list snapshot, a graphic test (`graphic_tests/`, regenerate goldens in the same commit — pixels move), WPT `tests/wpt/css/css-backgrounds/{parsing,}border-image-*` quick check (`CSS.supports`, computed values).
6. Docs: CSS-SPECS.md row (⬜ → 🟡/✅, list limits: no fallback to border-style on a broken URL, no animation of slice/width/outset/source if skipped), CAPABILITIES.md, BUG-492 → fixed per `docs/doc-sync.md`; then `/lumen-task-finish`, delete the pointer line from STATUS-P4.MD.

Gate before the closing commit: `cargo clippy -p lumen-layout -p lumen-paint --all-targets -- -D warnings`, targeted tests, full `python graphic_tests/run.py --continue-on-fail`.
