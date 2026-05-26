In progress: fix-list-markers-test32  branch: p5-fix-list-markers-test32
Next step: fix ListStylePosition::Inside in lay_out_block  box_tree.rs:1804

Role: P5 owns ALL bug fixes across the entire codebase.
  P1/P2/P3/P4 do NOT fix bugs — they build new features only.
  When P1–P4 discover a bug while implementing: add it to BUGS.md as OPEN,
  add a // BUG-NNN comment at the site, move on. P5 picks it up.

  P5 workflow:
  1. Run `python graphic_tests/run.py --continue-on-fail` → identify failing tests
  2. Pick the highest-deviation OPEN bug from BUGS.md
  3. Read the relevant crate with targeted grep/SYMBOLS.md lookup
  4. Fix + add regression test + update BUGS.md (OPEN → FIXED <date>)
  5. Clippy clean → cargo test → commit

Next:
- fix-list-markers-test32: TEST-32 8.61% — маркеры списков позиционированы неверно; layout/src/box_tree.rs BoxKind::Marker
- fix-direction-rtl-test27: TEST-27 9.35% — RTL text alignment; layout TextAlign::Start/End mirror logic
- fix-border-style-dashed: TEST-21 — border-style dashed/dotted алгоритм (текущий вариант оставляет артефакты)

Queue (новые баги по мере появления):
- Запускать `python graphic_tests/run.py --continue-on-fail` после каждого мержа P1–P4
- Новые провалы → добавить в BUGS.md + в этот Queue
- Следить за регрессиями: если ранее PASS тест упал → приоритет выше нового бага

Recent: fix-bug036-border-radius-pct — BUG-036 FIXED 2026-05-26 (border_*_radius поля f32→Length; parse_radius_length откладывает % до paint-time; CornerRadii::from_style_and_box резолвит по border-box; 20/20 тестов)
Previous: fix-bug028-resize — BUG-028 FIXED 2026-05-26 (guard Resized(0,0) в shell + defensive guard в relayout(); root cause BUG-027 уже устранён)
Previous: fix-bug020-overflow-axis — BUG-020 FIXED 2026-05-26 (CSS Overflow L3 §2.1 visible→auto coercion в compute_style; TEST-14: 1.70%→0.03% PASS)
Previous: fix-bug023-opacity-aa — BUG-023 FIXED 2026-05-26 (premultiplied alpha double-mult в composite shader; TEST-13: 2.20%→0.24% PASS)
