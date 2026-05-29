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
| 1 | Следующая задача из roadmap — см. `lumen-plan.md` Phase 2-3 (Track P1) | any | ? | — |

---

## Recent merges

- **p1-scroll-snap** ✅ 2026-05-29 — CSS Scroll Snap L1 algorithm stub в `lumen-layout`: `SnapPoint` + `SnapContainer` + `collect_snap_containers(root)` + `find_snap_target(container, current_scroll, target_scroll)` в `lib.rs`. mandatory/proximity strictness, `scroll-snap-stop: always` barrier, NodeId dedup. 10 unit-тестов. CSS-парсинг уже был в ComputedStyle (P4). STATUS-P4.md "Needs wiring" обновлён для P3 shell integration.
- **p1-roadmap-audit** ✅ 2026-05-29 — Синхронизация маркеров в `lumen-plan.md`: 18 позиций ⬜→✅ (Composite glyphs / HTTP+TLS / lumen-driver / Tab lifecycle инварианты / 8A.1 / 8A.2 / 8C.2 / 10B-10G / 10E.3). Crate descriptions: lumen-layout += StickyBox + image_gating; lumen-storage += IdbBackend. CLAUDE.md ext traits обновлён.
- **p1-indexeddb-persist** ✅ 2026-05-29 — IndexedDB persistence: новый трейт `IdbBackend` (`lumen-core::ext`) + impl `IdbStore` поверх `StorageBackend` (`lumen-storage`, работает с in-memory и SQLite). JS-шим сериализует все базы origin в один tagged-JSON снимок (Date сохраняются как `{__idb_date__: ms}`), `_lumen_idb_persist` пишет после каждого мутирующего flush, `_lumen_idb_load` восстанавливает при init → базы переживают reload. Read-only транзакции не пере-сохраняют (флаг `_idb_dirty`). Shell подключает общий `InMemoryStorage` (на процесс, как localStorage) + per-origin `IdbStore`; диск (SQLite) — замена бэкенда в одну строку. install_dom получил параметр `idb_backend`. 5 JS-тестов персистентности + 7 storage-тестов (267 JS, 267 storage). Обнаружен pre-existing BUG-044 (shell не компилируется: non-exhaustive match по DisplayCommand от P2-мерджей).
- **p1-indexeddb** ✅ 2026-05-29 — IndexedDB (Indexed Database API 3.0), чистый JS-шим в `WEB_API_SHIM` (без native-биндингов): `indexedDB.open/deleteDatabase/databases/cmp`, `IDBDatabase`/`IDBTransaction`/`IDBObjectStore`/`IDBIndex`/`IDBCursor`/`IDBKeyRange`/`IDB(Open)Request`. CRUD + unique/multiEntry индексы + курсоры (next/prev/unique, continue/advance/update/delete) + key range; key-порядок number<date<string<array; dotted/array keyPath; autoIncrement. Отложенная модель: действия запросов выполняются при dispatch в FIFO-порядке, события через `_lumen_idb_flush()`. In-memory (persistence — отдельная задача в Next). 18 тестов, всего 262 в lumen-js.
- **p1-image-viewport-gating** ✅ 2026-05-29 — `gate_image_requests(root, viewport, scroll_x, scroll_y)` в `lumen_layout::image_gating`: HashSet<NodeId> изображений в viewport ± 2 экрана. AABB-пересечение в document-space координатах. 7 интеграционных тестов.
- **p1-font-variation-wiring** ✅ 2026-05-29 — `Font::advance_width_varied(glyph_id, hmtx, coords)` применяет HVAR delta к advance width в `rasterize_and_insert`; gvar deltas для outline уже работали. `// CSS: font-variation-settings` комментарии в `TextMeasurer` и `measure_text_w` для P4. 4 новых теста (3 unit + 1 integration). 309+13+6 тестов lumen-font проходят.
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
