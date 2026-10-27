# p4-border-image — handoff (WIP, branch `p4-border-image`)

Task: `border-image` / `border-image-{source,slice,width,outset,repeat}` (CSS-SPECS.md row, BUG-492). Work in the main checkout (use_worktree off).

## Done

**Layout (commit df005c541):** `crates/engine/layout/src/style/values/border_image.rs` (types, parsers, computed serialisation), `ComputedStyle::border_image`, `apply/paint.rs`, `css_wide.rs`, `selector_query.rs`, `image_requests.rs`, CSSOM canon. `cargo test -p lumen-layout --lib border_image` — 5 passed.

**Paint (this WIP commit):**
- `paint/src/border_image.rs` — `BorderImageSpec`/`SliceValue`/`WidthValue`/`RepeatMode` (serde) + pure `border_image_quads(spec, area, img_w, img_h)` (§6.2–6.5: slice clamp, width scale-down, stretch/repeat/round/space, `fill`); 8 unit tests pass.
- `DisplayCommand::DrawBorderImage { area, src, spec }` wired in: `commands.rs` (variant, `variant_name`, `bounds`, roundtrip sample + names list), `translate.rs`, `fingerprint.rs` (x2), `serialize.rs`, `shell/display_list_metrics.rs` (x2), `shell/frames.rs` (rekey), `display_list/tests/images_media.rs`.
- Backends: `cpu_raster.rs` (via `blit_image_into_rect`), `renderer.rs` (wgpu `push_image_quad`, UV = slice/size), `femtovg_backend.rs` (`Paint::image` + path per quad).
- Emit: `display_list/border_image_emit.rs` `emit_border_image(b, dpr, out) -> bool`; hooked in `walk.rs` at every `has_border` site (`if has_border && !emit_border_image(..)`). URL (and `image-set()`) → `DrawBorderImage`; gradient → per quad `PushClipRect(dst)` + `Draw*Gradient` on the mapped full-image rect; cross-fade/paint()/unknown → `false` (plain `border-style` border).
- Tests: `cpu_raster/tests/layers_borders_images.rs` (2), `display_list/tests/background_and_layers.rs` (4) — all 14 `border_image` tests in `lumen-paint --lib` pass.
- Graphic test `graphic_tests/170-border-image.html` + entry in `run.py` `TESTS` + `COVERAGE.md` row (data-URI SVG source; 6 cells: stretch / repeat / round / space / fill+width+outset / gradient). **Not yet run against Edge.**

## Next

1. Build the binary: `cargo build --profile dev-release -p lumen-shell` — takes > 10 min incl. link; run it with `timeout 600000` on the Bash tool and **no inner `timeout 595`** (it killed the linker, exit 143). Disk: free ~49 GB after `rm -rf target/debug/incremental` (a `cargo check --workspace --all-targets` filled the disk once — don't repeat it).
2. Look at the page: `target/dev-release/lumen.exe --screenshot ... graphic_tests/170-border-image.html` (see `docs/automation.md`); check all 6 cells visually; then `LUMEN_PROFILE=dev-release python graphic_tests/run.py --only 170` (needs Edge screenshot), fix geometry/colour diffs (>0.5 % = fail; if only SVG-resampling noise, investigate before adding to `KNOWN_DEBTORS_IPC`). Gradient cell: Edge treats the gradient image as area-sized with slice 12 px.
3. Regenerate goldens in the same commit: `SAVE_CPU_SNAPSHOTS=1 cargo test -p lumen-driver --features cpu-render cases::snapshot_cpu` (adds `snapshots/cpu/170*.png`), `python graphic_tests/dump_golden.py`, `UPDATE_SNAPSHOTS=1 cargo test -p lumen-paint --test all` only if a `.snap` drifts; full `python graphic_tests/run.py --continue-on-fail` (foreground, focused window) and commit `results/*.json`.
4. WPT quick check: `tests/wpt/css/css-backgrounds/*border-image*` (`CSS.supports`, computed values via `getComputedStyle`).
5. Gate: `cargo clippy -p lumen-layout -p lumen-paint -p lumen-shell --all-targets -- -D warnings`; `cargo check --workspace` (no `--all-targets`) to catch other exhaustive `DisplayCommand` matches (only paint+shell were checked).
6. Docs in the same commit: CSS-SPECS.md row (⬜ → 🟡/✅; limits: no fallback to `border-style` when the URL fails to load, no animation of slice/width/outset/source, `border-radius` ignored as spec says, replaced-element boxes only via the shared `has_border` hooks, bilinear bleed across slice seams), CAPABILITIES.md, BUG-492 → BUGS-FIXED per `docs/doc-sync.md`; then `/lumen-task-finish`, delete the pointer line from STATUS-P4.MD.
