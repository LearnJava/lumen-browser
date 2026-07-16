# BUG-289 — vertical `writing-mode` InlineRun text never actually painted (real pages)

**Статус:** FIXED 2026-07-16
**Компонент:** paint (`display_list.rs::emit_inline_run`)
**Найден:** 2026-07-16, P3-vertical Срез 5 — первый end-to-end graphic-тест на реальном DOM (`graphic_tests/145-writing-mode.html`)

## Симптом

`graphic_tests/145-writing-mode.html` (6 боксов: `vertical-rl`/`vertical-lr` ×
`mixed`/`upright`/`sideways`) рендерился сломанным: три `vertical-rl` бокса
были полностью пустыми (фон без единого глифа), `vertical-lr` боксы
показывали текст, наложенный горизонтально поверх соседних колонок (слово
выходило за границы своего 152px бокса).

## Расследование

Слой layout (`lumen-layout::vertical`) был корректен — `--dump-layout`
подтвердил, что каждый `InlineRun` получал верный `rect` (например
`x=141.80 y=59.80 w=35.20 h=218.12`: колонка шириной `em·line-height`,
прижатая к правому краю бокса для `vertical-rl`, высотой — суммой
вертикальных экстентов слов). Баг был чисто в paint: `emit_inline_run`
(единственное место, конвертирующее `InlineFrag` в `DisplayCommand::DrawText`
для реального DOM) не проверяло `writing_mode` вообще и всегда трактовало
`frag.x` как физический X-офсет, `frag.width` как физическую ширину, а
`line_idx*line_h` как физический Y — корректно только для горизонтального
текста.

`wrap_inline_run_vertical` (layout) намеренно переиспользует те же поля с
другим физическим смыслом для вертикальных режимов: `frag.x` — это
накопленный курсор ВДОЛЬ inline-оси (физический Y, сверху вниз), `frag.width`
— собственный экстент фрагмента вдоль этой же оси (физическая высота). Ни
один из паrichных unit-тестов (`vertical.rs`, `cpu_raster.rs`,
`display_list.rs` — Срезы 1–3 этой задачи) этого не поймал, потому что все
они либо проверяли `LayoutBox`/`InlineFrag` напрямую (layout-only), либо
вызывали `rasterize_text`/`rasterize_cpu` с вручную сконструированными
`DisplayCommand::DrawText` (paint-only) — ни один не гонял реальный
`InlineRun → emit_inline_run → DrawText` путь для vertical writing mode.
`text-orientation`-поворот глифов (Срезы 1–3) был технически корректен, но
не имел смысла на практике: реальные `DrawText` для вертикального текста
никогда не получали правильную геометрию.

## Фикс (2026-07-16)

`emit_inline_run` теперь при `b.style.writing_mode != HorizontalTb` делегирует
в новую `emit_inline_run_vertical`: для каждой «строки» (`lines[i]` — одна
обёрнутая колонка вдоль block-оси) вычисляет `column_x` — `b.rect.x` для
колонки 0 (уже верно размещённой `lay_out_vertical_inline_run`/курсором
блок-укладки), `± i·col_width` для последующих колонок (влево для
`vertical-rl`/`sideways-rl`, вправо для `vertical-lr`/`sideways-lr`); для
каждого `frag` строит `rect = (column_x, b.rect.y + frag.x, col_width,
frag.width)` — читая те же поля, но с их РЕАЛЬНЫМ (вертикальным) физическим
смыслом. `text_orientation` пробрасывается безусловно (сегмент уже внутри
vertical-ветки).

Сознательно не перенесено на эту ось (Phase 0, тот же класс уже
задокументированных пробелов горизонтального пути): `vertical-align`
(`frag.y_offset`), inline-замещаемый контент (изображения) внутри
вертикального текста, `::selection`-подсветка, `text-overflow: ellipsis`.

`cargo test -p lumen-paint --features cpu-render,backend-wgpu` — 1093/1093
зелёных (регрессий нет — исправление затрагивает только vertical-ветку,
ранее не достижимую из реального DOM никаким существующим тестом).
`cargo clippy -p lumen-paint --all-targets --features cpu-render,backend-wgpu -- -D warnings` чист.

## Остаток

Визуально после фикса: `vertical-rl`/`vertical-lr` × `mixed`/`sideways`
рендерят читаемый повёрнутый/upright-CJK текст, как ожидается. `upright`
режим для латиницы выглядит урезанным относительно Edge (Edge раскладывает
каждый символ ИНДИВИДУАЛЬНО по вертикали; Lumen продолжает использовать
пословный экстент как аванс — уже задокументированный отдельный пробел,
"`Upright`'s per-glyph vertical advance is a separate follow-up", см.
`docs/tasks/ph3-writing-mode-vertical.md`) — вне скоупа этого фикса и этой
задачи. Остаточный diff TEST-145 после этого фикса — `KNOWN_DEBTOR`
[BUG-290](BUG-290-OPEN.md) (font-parity + upright per-character advance
approximation, не layout/paint-геометрия).
