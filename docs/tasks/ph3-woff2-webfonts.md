# Ph3 — WebFonts (@font-face + WOFF2)

**Developer:** P2
**Branch:** `p2-ph3-woff2-webfonts`
**Size:** L
**Crates:** `lumen-font`, `lumen-css-parser` (P4 handoff), `lumen-shell`
**Phase:** 3 (v1.0 target)

---

## Status

**Phase 3 future item.** The infrastructure is largely already built as a Phase 2 side-effect (`PH3-19` tags in shell). The pieces that exist today:

- WOFF2 and WOFF1 decoders — `crates/engine/font/src/woff2.rs` (DONE)
- `FontRegistry` with in-memory byte store and `register_from_bytes` — `crates/engine/font/src/font_registry.rs` (DONE)
- `@font-face` parsing (`FontFaceRule`, `FontFaceSource`, `FontFaceSourceKind`) — `crates/engine/css-parser/src/parser.rs:1097` (DONE)
- Two-pass load in shell (`local()` sync, `url()` async via `PendingWebFont`) — `crates/shell/src/main.rs:4002` (DONE)
- `FontLoaded` event → `register_from_bytes` → `relayout` (FOUT swap) — `crates/shell/src/main.rs:7335` (DONE)
- `FemtovgBackend::load_font_by_path` reads `provider.read_face_bytes` for virtual paths — `crates/engine/paint/src/backends/femtovg_backend.rs:1136` (DONE)

What is **incomplete / missing** (the actual Phase 3 work):

- `font-display` semantics: parsed into `FontFaceRule::display` (`crates/engine/css-parser/src/parser.rs:1110`) but never consumed; current behavior always uses FOUT/swap regardless of value.
- Weight/style range descriptors: `font-weight: 100 900` (range syntax, CSS Fonts L4 §4.6) stored as raw string, not parsed as a numeric range. The shell parses only single keywords/numbers (`parse_font_weight` at `crates/shell/src/main.rs:4069`).
- Only the first `url()` source is queued per rule (`find(|s| s.kind == Url)`); if that fetch fails the next `url()` is not retried (CSS §4.1 says try in order).
- `unicode-range` is passed to femtovg/`MultiFontMeasurer` but not enforced by the CSS font-matching algorithm when selecting which face to use for a given codepoint.
- `FontRegistry::pick_face` delegates directly to `SystemFontIndex::pick_face` for custom faces via `FontProvider::lookup_faces`; CSS Fonts L4 §5.2 weight-distance matching for `@font-face` faces uses the same scoring but has not been audited for the custom-face path.
- Integration test with a real WOFF2 file fetched over HTTP does not exist.

---

## Goal

Complete Phase 3 quality-bar for `@font-face` + WOFF2:

1. Implement `font-display` swap/block/fallback/optional semantics (timeout-based FOIT vs FOUT).
2. Parse weight/style range descriptors (`100 900`) as numeric ranges for correct CSS Fonts L4 face selection.
3. Implement source-list fallback: if the first `url()` fetch fails, try the next one.
4. Enforce `unicode-range` in font-matching: only use a face when the text codepoint falls within its declared range (CSS Fonts L4 §3.2).
5. Add an integration test against a real WOFF2 font file.

---

## Current state (file:line)

### Container formats

`crates/engine/font/src/lib.rs:85` exports `maybe_decode_font`, `decode_woff1`, `decode_woff2`, `is_woff1`, `is_woff2`.

The SFNT parse entry point is `Font::parse(data: &[u8])` at `crates/engine/font/src/face.rs:98`. It expects a raw sfnt byte slice (TTF/OTF magic `0x00010000`, `OTTO`, or `true`). WOFF/WOFF2 containers must be decoded to sfnt first; the shell does this via `lumen_font::maybe_decode_font` at `crates/shell/src/main.rs:7030`.

WOFF2 decoder (`crates/engine/font/src/woff2.rs`):
- Magic detection: `is_woff2` at line 18, `is_woff1` at line 23.
- WOFF2 full decode (Brotli + transformed `glyf`/`loca` reconstruction): `decode_woff2` at line 483. Uses `brotli-decompressor = "5"` (already in `Cargo.toml`).
- WOFF1 decode (per-table zlib via `zune-inflate`): `decode_woff1` at line 699.
- Unified dispatcher: `maybe_decode_font` at line 764 — returns `Ok(None)` for raw sfnt (caller uses as-is).

### @font-face CSS parser

`crates/engine/css-parser/src/parser.rs`:
- `FontFaceRule` struct at line 1097: fields `family`, `sources`, `weight`, `style`, `stretch`, `display`, `unicode_range`, `variant`, `feature_settings`, `variation_settings`. All descriptors except `family` and `sources` are stored as raw strings.
- `FontFaceSource` at line 1122: `kind: FontFaceSourceKind`, `value: String`, `format: Option<String>`.
- `FontFaceSourceKind` at line 1131: `Url` / `Local`.
- `parse_font_face_body` at line 2444 populates all descriptors.
- `font_faces: Vec<FontFaceRule>` field on `Stylesheet` at line 836.
- Tests at lines 5973–6155 cover parse round-trips.

### Font load + registration in shell

`crates/shell/src/main.rs`:
- `PendingWebFont` struct at line 179: `family`, `weight`, `style`, `unicode_range_str`, `url`.
- `load_font_faces` at line 4010: two-pass — `local()` sync, first `url()` queued.
- `apply_loaded_page` at line 7011: spawns one thread per `PendingWebFont`; uses `fetch_image_bytes` for HTTP, then `maybe_decode_font`, then `Font::parse` validation, then sends `LoadEvent::FontLoaded`.
- `LoadEvent::FontLoaded` handler at line 7335: calls `FontRegistry::register_from_bytes`, updates renderer's `FontProvider`, calls `self.relayout()`.
- **Gap**: `font-display` value from `FontFaceRule::display` is never read (lines 4055–4063 build `PendingWebFont` without passing `display`).
- **Gap**: only `rule.sources.iter().find(|s| s.kind == Url)` at line 4055 — single source, no fallback chain.

### FontRegistry (lumen-font)

`crates/engine/font/src/font_registry.rs`:
- `register_from_bytes(family, weight, style, bytes)` at line 52: stores sfnt bytes under virtual path `@font-face:<family>/<weight>/<style>`.
- `read_face_bytes(path)` at line 155: returns in-memory bytes for virtual paths; `None` for system paths (renderer falls back to `fs::read`).
- `FontProvider` impl at line 124: merges system + custom faces in `lookup_faces`, `list_families`, `lookup_family`.

### FemtovgBackend font loading

`crates/engine/paint/src/backends/femtovg_backend.rs`:
- `load_font_by_path` at line 1139: calls `provider.read_face_bytes(path)` first, then `fs::read`. Calls `canvas.add_font_mem(&bytes)` — femtovg takes ownership of raw sfnt bytes and does its own shaping.
- `resolve_font_chain` at line 1160: iterates CSS families, calls `provider.pick_face(fam, weight, style)`, then appends bundled Inter and curated system fallbacks.
- **Key constraint from memory `text_render_paths_fork`**: the window path uses femtovg (`FemtovgBackend::load_font_by_path` → `canvas.add_font_mem`); the CPU-snapshot/wgpu path uses lumen-font (`Renderer` in `renderer.rs`). A downloaded web font must be registered via BOTH paths to render correctly everywhere. `register_from_bytes` already satisfies the lumen-font path (bytes go into `FontRegistry::bytes_store`). The femtovg path picks them up lazily through `load_font_by_path` which reads `provider.read_face_bytes` — so a single `FontRegistry` instance shared via `Arc<dyn FontProvider>` covers both paths as long as `set_font_provider` is called after registration (done at line 7062 / 7349).

### Dependencies

`crates/engine/font/Cargo.toml`:
- `brotli-decompressor = "5"` — already present. WOFF2 requires Brotli; this crate is zero-unsafe pure Rust.
- `zune-inflate = "0.2"` — already present. Used for WOFF1 per-table zlib.
- No new dependencies needed for the missing features.

---

## Architecture

```
CSS source
  └─ css-parser: parse_font_face_body()
       → FontFaceRule { family, sources: [url(foo.woff2) format("woff2"), ...],
                        weight, style, display, unicode_range }

Shell: load_font_faces()           [two-pass]
  Pass 1: local() → SystemFontIndex::pick_face → fs::read → register_from_bytes
  Pass 2: url()   → PendingWebFont queued

Shell: apply_loaded_page()
  └─ per PendingWebFont: spawn thread
       fetch_image_bytes(url) → raw bytes
       maybe_decode_font(raw) → sfnt bytes   [woff2.rs]
       Font::parse(sfnt)      → validation   [face.rs]
       LoadEvent::FontLoaded { bytes, ... }

Shell: FontLoaded handler
  ├─ FontRegistry::register_from_bytes(family, weight, style, sfnt_bytes)
  ├─ renderer.set_font_provider(Arc<FontRegistry>)
  └─ relayout()                              [FOUT swap]

FemtovgBackend (window render path)
  └─ resolve_font_chain(families, weight, style)
       └─ provider.pick_face(family, weight, style)   [FontRegistry]
            └─ load_font_by_path(virtual_path)
                 └─ provider.read_face_bytes(virt_path) → sfnt bytes
                      canvas.add_font_mem(bytes) → FontId

lumen-font Renderer (CPU/wgpu render path)
  └─ reads FontProvider::read_face_bytes(virt_path) → sfnt bytes
       Font::parse(sfnt)
       Rasterizer::rasterize(glyph_id)
```

---

## Cross-team boundary (P4)

`@font-face` parsing is P4's domain. The current parser (`parse_font_face_body` at `crates/engine/css-parser/src/parser.rs:2444`) already covers the descriptors needed for Phase 3. The remaining P4 handoff items are:

1. **Weight/style range parsing** — `font-weight: 100 900` (CSS Fonts L4 §4.6). The shell's `parse_font_weight` helper at `crates/shell/src/main.rs:4069` handles single keywords/numbers. The raw string is in `FontFaceRule::weight`. P4 should either:
   - Add `weight_range: Option<(u16, u16)>` to `FontFaceRule` and populate it in `parse_font_face_body`, or
   - Document that the shell should parse the range from the raw string.
   Mark this handoff: `// CSS: font-weight range (@font-face, CSS Fonts L4 §4.6)` at line 4069.

2. **`font-display` value** — stored as `FontFaceRule::display: Option<String>` at line 1110, never consumed. P4 may optionally add a typed enum (`FontDisplay { Auto, Block, Swap, Fallback, Optional }`), but the Phase 3 implementation can also parse it inline in the shell.

---

## Entry points (real file:line)

| Symbol | File:line | Status |
|---|---|---|
| `woff2::maybe_decode_font` | `crates/engine/font/src/woff2.rs:764` | Done |
| `woff2::decode_woff2` | `crates/engine/font/src/woff2.rs:483` | Done |
| `woff2::decode_woff1` | `crates/engine/font/src/woff2.rs:699` | Done |
| `Font::parse` | `crates/engine/font/src/face.rs:98` | Done |
| `FontRegistry::register_from_bytes` | `crates/engine/font/src/font_registry.rs:52` | Done |
| `FontRegistry::read_face_bytes` | `crates/engine/font/src/font_registry.rs:155` | Done |
| `FontFaceRule` struct | `crates/engine/css-parser/src/parser.rs:1097` | Done |
| `parse_font_face_body` | `crates/engine/css-parser/src/parser.rs:2444` | Done |
| `PendingWebFont` struct | `crates/shell/src/main.rs:179` | Done |
| `load_font_faces` | `crates/shell/src/main.rs:4010` | Partial (no source-list fallback, no display) |
| `apply_loaded_page` web-font spawn | `crates/shell/src/main.rs:7011` | Partial (no display-based FOIT) |
| `LoadEvent::FontLoaded` handler | `crates/shell/src/main.rs:7335` | Done |
| `parse_font_weight` | `crates/shell/src/main.rs:4069` | Partial (single value only, no range) |
| `FemtovgBackend::load_font_by_path` | `crates/engine/paint/src/backends/femtovg_backend.rs:1139` | Done |
| `FemtovgBackend::resolve_font_chain` | `crates/engine/paint/src/backends/femtovg_backend.rs:1160` | Done |
| Add `FontDisplay` enum to `FontFaceRule` | `crates/engine/css-parser/src/parser.rs:1109` | **Proposed** |
| Add `weight_range: Option<(u16, u16)>` to `FontFaceRule` | `crates/engine/css-parser/src/parser.rs:1104` | **Proposed** |
| Extend `PendingWebFont` with `display`, source list | `crates/shell/src/main.rs:179` | **Proposed** |
| Source-list fallback retry loop | `crates/shell/src/main.rs:4054` | **Proposed** |
| `unicode-range` enforcement in face selection | `crates/engine/font/src/font_registry.rs` | **Proposed** |

---

## Steps

### Step 1 — Source-list fallback (shell, S)

Change `load_font_faces` (`crates/shell/src/main.rs:4054`) to collect all `url()` sources per rule (not just `find` the first). Store them as `Vec<String>` on `PendingWebFont`. In the async fetch loop (`crates/shell/src/main.rs:7017`), iterate `url_sources` in order and stop at first success (CSS Fonts L4 §4.1).

### Step 2 — `font-display` typed enum (css-parser, P4 handoff)

Add `FontDisplay` enum to `crates/engine/css-parser/src/parser.rs` near line 1109:
```rust
pub enum FontDisplay { Auto, Block, Swap, Fallback, Optional }
```
Replace `display: Option<String>` with `display: FontDisplay` (default `Auto`). Parse in `parse_font_face_body` at line 2475. Update all `FontFaceRule` construction sites.

### Step 3 — `font-display` semantics (shell, M)

Pass `FontDisplay` through `PendingWebFont`. In the async fetch thread:
- `Swap` (current de-facto behavior): show fallback immediately; swap when loaded. No change needed.
- `Block`: hide text (render invisible) for up to 3 s; swap when loaded; show fallback if timeout.
- `Fallback`: block for 100 ms; then show fallback; swap only within 3 s; after 3 s stick with fallback.
- `Optional`: block for 100 ms; browser may skip swap entirely (use fallback permanently).
- `Auto`: browser-defined; implement as `Swap` for simplicity.

Implementation sketch: introduce a timeout in the spawn thread based on `display` value; send a `LoadEvent::FontDisplayTimeout { family, fallback_only: bool }` after the block period so the shell can switch rendering mode before the font arrives.

### Step 4 — Weight/style range parsing (css-parser + shell, P4 handoff)

P4 adds `weight_range: Option<(u16, u16)>` to `FontFaceRule` (line 1104). Shell's `register_from_bytes` loop (`crates/shell/src/main.rs:4027`) should register one `FaceRecord` per weight in range (or store the range in `FaceRecord`). For Phase 3 a simpler approach: store midpoint as `weight` and add `weight_min`/`weight_max` to `FaceRecord` in `lumen-core`; update `SystemFontIndex::pick_face` to account for range in CSS Fonts L4 §5.2 weight-distance algorithm.

### Step 5 — `unicode-range` enforcement (lumen-font, M)

`FontRegistry` already passes `unicode_range: Vec<UnicodeRange>` through `LoadedWebFont` (`crates/shell/src/main.rs:198`) but does not store it alongside `FaceRecord`. Add `unicode_range: Vec<UnicodeRange>` to `FaceRecord` in `lumen-core`. Populate it in `register_from_bytes`. Modify `FontProvider::pick_face` / `lookup_faces` callers (femtovg `resolve_font_chain` at line 1174) to skip a face if the requested codepoint falls outside its declared ranges (use `codepoint_in_ranges` from `crates/engine/font/src/unicode_range.rs`).

### Step 6 — Integration test (lumen-font, S)

Add `crates/engine/font/tests/woff2_real.rs`: download a public-domain WOFF2 (e.g. subset of Noto Sans) or use a tiny synthetic one generated with `woff2` CLI tool. Verify:
- `is_woff2` returns true.
- `decode_woff2` succeeds.
- `Font::parse(decoded_bytes)` succeeds.
- At least one glyph can be rasterized.

Also add a WOFF1 round-trip test.

---

## Dependencies

| Crate | Dep | Justification |
|---|---|---|
| `lumen-font` | `brotli-decompressor = "5"` | Already in `Cargo.toml`. WOFF2 containers use Brotli compression (W3C WOFF2 spec §4). Pure Rust, zero-unsafe; category: permanent (WOFF2 is the dominant web font format). |
| `lumen-font` | `zune-inflate = "0.2"` | Already in `Cargo.toml`. WOFF1 uses per-table zlib/deflate. Pure Rust; category: permanent. |

No new dependencies needed for Phase 3 completion. Both decompressors are permanent (they cover the only two web font container formats).

---

## Tests

| Test | Where | What |
|---|---|---|
| `woff2::decode_woff2_rejects_bad_magic` | `crates/engine/font/src/woff2.rs:810` | Already exists |
| `woff2::maybe_decode_none_for_raw_sfnt` | `crates/engine/font/src/woff2.rs:803` | Already exists |
| `font_registry::register_and_lookup` | `crates/engine/font/src/font_registry.rs:176` | Already exists |
| `woff2_real_font` integration | `crates/engine/font/tests/woff2_real.rs` | **Proposed** |
| `load_font_faces_source_fallback` | `crates/shell` (unit, mocked fetch) | **Proposed**: first `url()` fails → second succeeds → font registered |
| `font_display_swap_registers_immediately` | `crates/shell` | **Proposed**: `font-display: swap` → FOUT path taken |
| `unicode_range_skips_out_of_range` | `crates/engine/font/src/font_registry.rs` | **Proposed**: face with `U+0041-005A` not selected for U+0061 |
| `weight_range_pick_nearest` | `crates/engine/font/src/font_registry.rs` | **Proposed**: `font-weight: 100 900` face selected for weight=300 |

---

## Definition of done

- [ ] Source-list fallback: if first `url()` fetch fails, the next source in the list is tried (CSS Fonts L4 §4.1).
- [ ] `font-display: swap` (current default), `block`, `fallback`, `optional` behave per spec timing (block period + swap period).
- [ ] `font-weight: 100 900` range syntax parsed; CSS Fonts L4 §5.2 weight-distance matching applies to ranged faces.
- [ ] `unicode-range` enforced in face selection: face skipped for codepoints outside declared ranges.
- [ ] `cargo clippy -p lumen-font --all-targets -- -D warnings` clean.
- [ ] `cargo clippy -p lumen-shell --all-targets -- -D warnings` clean.
- [ ] All proposed tests pass.
- [ ] Integration test exercises a real WOFF2 decode → rasterize round-trip.
- [ ] `CAPABILITIES.md` updated: `@font-face url() → WOFF2` row changed from partial to ✅.
- [ ] `CSS-SPECS.md`: `@font-face` / `font-display` rows updated.
