# STATUS-P1 — Feature Development

**Developer:** Программист 1 (Feature development — any subsystem from roadmap)

---

## In progress
_(none)_

## Next step
1. Read [lumen-plan.md](lumen-plan.md) Track P1 section (§P1 roadmap)
2. Phase 0 ✅ + Phase 1 ✅ complete — pick first ⬜ item from Phase 2-3 or coordinate with P2/P3
3. Create branch: `git checkout -b p1-<task-name>`
4. In first commit: update this file with "In progress: <task>" + branch name

---

## Recent merges

- **p1-sticky-layout** ✅ 2026-05-29 — `StickyBox` + `collect_sticky_boxes()` + `compute_sticky_offset()` в `lumen-layout/src/lib.rs`. Algorithm stub: sticky в normal flow; collect собирает static_rect и px-инсеты (non-px → None); compute — чистая функция `(scroll_x, scroll_y, vp_w, vp_h) → (dx, dy)`. Дедупликация по NodeId. P4 подключает CSS-инсеты, P3 — scroll-события. 9 unit-тестов. STATUS-P4.md "Needs wiring" обновлён.
- **p1-hyphenation-provider** ✅ 2026-05-29 — `KnuthLiangHyphenation`: реальный `HyphenationProvider` через provisional `hyphenation = "0.8"` (Knuth–Liang, TeX-словари). 11 локалей (en/ru/de/fr/uk/nl/es/pt/it/pl/cs). Подключён в `lumen-shell` через `layout_measured_hyp`. 88 unit + 6 integration tests.
- **p1-phase1-status-sync** ✅ 2026-05-28 — Sync lumen-plan.md Phase 1 statuses with actual code state: 8G.1–8G.3 (lumen-a11y-full, 125 tests), 10B (DOM arena serialization, `Document::to_bytes`/`from_bytes`), 10D.1/10D.2 (layout/paint pure audit), 9D.1 (Canvas noise generator, 20 tests), 9D.2 (GpuFingerprint, 5 tests), 10F (LayerCache LRU, 7 tests), 10G (glyph atlas eviction, 4 tests). All Phase 1 ⬜ → ✅.

---

## Notes

- **Coordinate with P2:** Check STATUS-P2.md before starting cross-domain work
- **CSS workflow:** If your algorithm needs a CSS property, add `// CSS: <property>` comment and note in STATUS-P4.md "Needs wiring"
- **Bug discovery:** Don't fix bugs — add to BUGS.md with next BUG-NNN number, continue feature work
- **All tasks tracked:** Use git branch prefix `p1-<task-name>` so parallel sessions don't duplicate

See CLAUDE.md for full workflow details.
