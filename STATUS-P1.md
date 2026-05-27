In progress: click-hint-overlay (7B.2) — collect_clickable_elements iterator  branch: p1-click-hint-overlay
Next step: define ClickableElement struct + collect_clickable_elements(LayoutBox, Document) fn  lumen-layout/src/clickable.rs

CSS rule: P1 does NOT implement CSS properties. P4 owns all CSS.
  P1 writes layout algorithms and box-tree structure only.
  When a new algorithm needs a CSS property → add // CSS: <prop> comment at
  the call site and add a line to STATUS-P4.md "Needs wiring".

Bug fixes rule: P1 does NOT fix bugs. Discovered bugs → add to BUGS.md + P5 picks up.

Recent:
- print-pdf-pagination (5++, Phase 1): PaginationContext + Page + PageFragment, paginate() algorithm for break-before/after/avoid, 7 unit tests, exports in lib.rs, clippy clean 2026-05-28
- bench-ram-axis (9G.5, ADR-008 performance gate): cross-platform RSS measurement (getrusage on Unix, GetProcessMemoryInfo on Windows), baseline.json established, UPDATE.md documentation 2026-05-28
- antidetect-webgl-normalize (9D.2, ADR-007 Layer 4): GpuFingerprint struct in paint/src/fingerprint.rs, from_adapter_info() normalization (always "WebKit"/"Generic GPU"), Renderer.gpu_fingerprint field, install_webgl_bindings() in js/src/webgl_bindings.rs stores _LUMEN_GPU_VENDOR/_RENDERER globals, 5 tests, P1 complete 2026-05-28 — P3 integration pending
- gpu-layer-lru (10F, Phase 1): LayerCache struct in paint/src/layer_cache.rs, LRU-tracked GPU layer metadata (LayerKey + LayerEntry), get_lru_candidates() for eviction, remove_keys() for memory reclaim, 256 MB default budget, 7 unit tests + Renderer integration, Phase 2 (10F.2) texture pool recycling pending 2026-05-28
- antidetect-canvas-randomization (9D.1): CanvasNoiseGenerator LCG RNG в canvas/src/fp_noise.rs, per-session deterministic XOR-noise R/G/B, Context2D::set_noise_generator() + get_image_data() stub для P3 JS-интеграции, 20 тестов 2026-05-28
- extras-p2 (5++): object-fit ✅ + variable fonts ✅ + Print PDF Phase 1 (pagination module) 2026-05-27
- glyph-atlas-eviction (10G.1): LRU tracking + get_lru_candidates() + remove_keys() для эвикции, 4 новых теста, Phase 1 завершена 2026-05-27
- fts-omnibox (8F.1, Wave 1): HistoryWithFts integration with lumen-storage::History — record_visit_with_text() + delete_with_fts() automatic sync hooks, 3 integration tests → 49 total PASS 2026-05-27
