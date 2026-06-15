# BUG-043

**Статус:** FIXED 2026-05-29
**Компонент:** paint
**Файл:** `paint/tests/snapshot_tests.rs, paint/src/display_list.rs`

## Описание

lumen-paint test suite красный (19 падений, не только 7): (1) 5 golden устарели — DrawText теперь несёт var=["opsz"=16] (font-optical-sizing 27fda15) → регенерированы; (2) overflow visible+hidden coercion (BUG-020) → visible computes to auto; auto = scroll-container, поэтому клип идёт через PushScrollLayer (p2-scroll-layer), обе оси к padding-box; 5 тестов (2 snapshot + 2 lib ordered_clip + чужой ordered_overflow_x_alone_triggers_clip) ждали PushClipRect/single-axis sentinel → переписаны под PushScrollLayer; (3) первая строка несёт half-leading 1.6px (CSS 2.1 §10.8.1), 5 baseline/wrap lib-тестов ждали line_y=0 → обновлены
