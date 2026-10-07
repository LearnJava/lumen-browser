# BUG-1388 — `contain-intrinsic-size` не даёт вклада во внутренние размеры родителя (`width: max-content`, flex)

**Статус:** OPEN
**Заведён:** 2026-10-07 (P2, WPT-RUN-14 срез 16, `css/css-overflow` + `css/css-sizing`)
**Область:** layout (`crates/engine/layout/src/box_tree/intrinsic.rs` — `min_content_outer_width`/`max_content_outer_width` не знают `contain: size` + `contain_intrinsic_width`; в потоке `layout_dispatch.rs` значение учтено)

## Симптом

`--dump-layout`, `#border{width:max-content;border:1px solid}`, внутри `#t{contain:size;contain-intrinsic-size:111px 222px;background:lightblue}`:

- получено: `#border` 2×224, `#t` 0×222;
- ожидается: `#border` 113×224, `#t` 111×222.

То же с `display:flex;width:max-content` вокруг `contain-intrinsic-size:50px 60px;contain:size` → ширина 0 (ожидается 50). С `display:inline-block` вместо блока ширина верна (50) — значит значение читается в потоке, но не в расчёте max-content родителя. Форма `auto <length>` (последний запомненный размер) разобрана и трактуется как длина (`style/computed.rs:923`: «behaviourally ignored»).

## Как найдено

WPT-RUN-14 срез 16: `css-sizing/contain-intrinsic-size/contain-intrinsic-size-001…033` (21 thick из 47 id), `auto-001…018` (17 id — запомненный размер, отдельная функциональность, см. `CSS-SPECS.md:199`).

## Что делать

Учесть `contain: size` + `contain-intrinsic-*` в intrinsic-размерах (`intrinsic.rs`). `auto` — «last remembered size» — отдельная работа: запоминать размер при последнем рендеринге; указатель на `CSS-SPECS.md:199`.

## Как проверить

`css/css-sizing/contain-intrinsic-size/contain-intrinsic-size-002.html`.
