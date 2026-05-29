# STATUS-P1 — Feature Development

**Developer:** Программист 1 (Feature development — any subsystem from roadmap)

---

## In progress
_(none)_

---

## Next

Ordered by impact. Pick the first unblocked item; update "In progress" before coding.

| # | Task | Crate(s) | Effort | Blocker |
|---|------|----------|--------|---------|
| 1 | `font-variation-settings` rasterizer wiring — применять gvar/HVAR deltas при растеризации; добавить `// CSS: font-variation-settings` на call site для P4 | `font`, `layout` | M | none |
| 2 | Image viewport-gating (10E.3) — layout декодирует изображения только для bbox ∈ viewport ± 2 экрана, `layout/src/image_gating.rs` | `layout`, `image` | M | none |
| 3 | Click-hint overlay iterator (7B.2 blocker) — публичный итератор по кликабельным элементам из `lumen-layout`; P3 подключает в shell | `layout` | S | none |
| 4 | Shadow DOM JS binding stubs — lifecycle callbacks для P3 (`connectedCallback`, `disconnectedCallback`, `attributeChangedCallback`) в `lumen-dom` | `dom` | M | P3 JS bindings |

---

## Recent merges

- **p1-lazy-io** ✅ 2026-05-29 — `loading="lazy"` через IntersectionObserver event source: `_lumen_init_lazy_images()` создаёт internal IO с rootMargin 1-viewport-height, `_lumen_deliver_lazy_images()` → no-op; добавлен `_parse_root_margin()` + rootMargin-aware delivery в IO; исправлен BUG-042 (QuickJsRuntime::resume stub). 244 JS-теста проходят.
- **p1-sticky-layout** ✅ 2026-05-29 — `StickyBox` + `collect_sticky_boxes()` + `compute_sticky_offset()` в `lumen-layout/src/lib.rs`. Algorithm stub: sticky в normal flow; collect собирает static_rect и px-инсеты (non-px → None); compute — чистая функция `(scroll_x, scroll_y, vp_w, vp_h) → (dx, dy)`. Дедупликация по NodeId. 9 unit-тестов. STATUS-P4.md "Needs wiring" обновлён.
- **p1-hyphenation-provider** ✅ 2026-05-29 — `KnuthLiangHyphenation`: реальный `HyphenationProvider` через provisional `hyphenation = "0.8"` (Knuth–Liang, TeX-словари). 11 локалей (en/ru/de/fr/uk/nl/es/pt/it/pl/cs). Подключён в `lumen-shell` через `layout_measured_hyp`. 88 unit + 6 integration tests.
- **p1-phase1-status-sync** ✅ 2026-05-28 — Sync lumen-plan.md Phase 1 statuses with actual code state: 8G.1–8G.3 (lumen-a11y-full, 125 tests), 10B (DOM arena serialization, `Document::to_bytes`/`from_bytes`), 10D.1/10D.2 (layout/paint pure audit), 9D.1 (Canvas noise generator, 20 tests), 9D.2 (GpuFingerprint, 5 tests), 10F (LayerCache LRU, 7 tests), 10G (glyph atlas eviction, 4 tests). All Phase 1 ⬜ → ✅.

---

## Notes

- **Coordinate with P2:** Check STATUS-P2.md before starting cross-domain work
- **CSS workflow:** If your algorithm needs a CSS property, add `// CSS: <property>` comment and note in STATUS-P4.md "Needs wiring"
- **Bug discovery:** Don't fix bugs — add to BUGS.md with next BUG-NNN number, continue feature work
- **All tasks tracked:** Use git branch prefix `p1-<task-name>` so parallel sessions don't duplicate

See CLAUDE.md for full workflow details.
