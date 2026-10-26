# BUG-1716 — остаток BUG-1527: `align-content` в multicol, перерасчёт после смены стиля предка, `position:relative` у inline-`<span>`

**Статус:** OPEN
**Заведён:** 2026-10-26 (P3, остаток [BUG-1527](BUG-1527-FIXED.md))
**Область:** layout (`crates/engine/layout/src/box_tree/multicol_trampoline.rs`; `box_tree/block_flow_trampoline.rs`, `incremental.rs`; inline-раскладка — [BUG-1711](BUG-1711-OPEN.md))

## Симптом

1. `css/css-align/multicol/align-content-multicol.html` (32 сабтеста) — `align-content` у multicol-контейнера не смещает содержимое колонок; ожидается выравнивание содержимого каждой колонки по свободному месту (CSS Box Alignment L3 §«Block Containers» для multicol). `block_align_content::applies` multicol исключает не явно: колонки раскладывает `multicol_trampoline`, `finish_frame` блочного потока для них не вызывается.
2. `css/css-align/blocks/align-content-block-002.html`, `-003.html` — все 17 сабтестов красные только из-за `offsetTop` у `.relpos` (`position: relative; top: -1.5em` на inline-`<span>`): отдаёт 15 вместо 0, `top` не применяется (BUG-1711). Эмулятор `box_tree/tests/align_self_wpt.rs` проверяет только боксы и на этих файлах чист.
3. Динамические `align-content-block-006…011` (7/17, 7/17, 2/17, 7/17, 7/17, 15/17) и `align-content-block-simple-height-change.html` (0/1): после смены стиля содержимое не перевыравнивается. Проба: `<div id=target style="height:200px"><div id=mid2 style="height:100%">…</div></div>`; `target.style.height='100px'` → `mid2.offsetHeight` остаётся 200 (ожидается 100), то есть процентная высота потомка не пересчитывается после смены `height` предка — без `align-content` вовсе. Прямая смена `align-content`/`height` самого блока работает.
4. `align-content-table-cell.html`: `align-content: baseline` против `vertical-align: baseline` — `offsetTop` 7.797174 и 7.797172 (расхождение 2e-6 из-за порядка сложения `f32`), `assert_equals` без допуска.

Не проверено: фрагментация (`align-content-block-break-*`, reftest-ы не входят в прогон `--all`).

## Как проверить

`python tests/wpt/run_report.py --binary <lumen.exe> --all --root css/css-align/blocks --recursive` и `--root css/css-align/multicol`; эмулятор — `LUMEN_ALIGN_WPT=<файлы> cargo test -p lumen-layout --lib survey -- --ignored --nocapture`.
