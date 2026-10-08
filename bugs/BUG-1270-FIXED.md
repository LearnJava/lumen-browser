# BUG-1270 — повёрнутый вертикальный текст рисуется на колонку левее своего места

**Статус:** FIXED 2026-10-08 (b08924bf5, BUG-553 срез 62)
**Заведён:** 2026-10-05 (P1, найден при LAYOUT-VFLOAT: reftest-проба `float-vlr-003.xht` не могла совпасть с плоским зелёным эталоном)
**Область:** paint (`cpu_raster.rs`, `renderer/glyph_raster.rs`, `backends/femtovg_backend.rs`)

## Симптом

`text-orientation: mixed` (по умолчанию) и `sideways` в `writing-mode: vertical-*`: латинский текст попадает не в свою
колонку, а в соседнюю слева. Раскладка верна — `--dump-display-list` даёт `DrawText (60.00, 60.00, 50.00, 100.00) "RA"`
для колонки `x = 60..110`, — но пиксели лежат в `x = 10..60`.

## Как воспроизвести

```html
<body style="margin:60px"><div style="font:50px/1 Ahem;writing-mode:vertical-lr;width:100px;height:100px;background:#ccc;color:green">RA</div>
```

`LOCALAPPDATA=<каталог с Microsoft/Windows/Fonts/Ahem.ttf> LUMEN_CPU_SYSTEM_FONTS=1 lumen --viewport 300x250 --screenshot out.png <файл>`:
зелёная полоса `x = 10..60` при сером боксе `x = 60..160` (без Ahem — те же `x = 10..60`, глифы Inter).

## Причина

`emit_inline_run_vertical` (`display_list/text_run.rs`) кладёт в `DrawText` `rect = (column_x, …, col_width, frag.width)`, где
`column_x` — левый край колонки. Бэкенды вращают прогон по часовой вокруг `(rect.x, rect.y)`:
`Transform::from_row(0, 1, -1, 0, rect.x, rect.y)` (`cpu_raster.rs`), `rotate_text_vertices_cw` (wgpu), `rotate_cw_transform`
(femtovg). Локальная ось `y` (вниз, `0..font_size`) уходит в `x' = rect.x - y`, то есть глиф занимает `[rect.x - font_size, rect.x]`.
Тест `rotate_cw_transform_makes_the_run_flow_downwards_from_the_column_origin` называет это «тело глифа левее начала колонки»
и принимает как должное. Прямые CJK-иероглифы в `rasterize_text_mixed` идут без вращения в `[rect.x, rect.x + font_size]` —
в одной колонке латиница и CJK расходятся на `font_size`.

## Что делать

Сдвинуть начало вращения на `(rect.width + font_size) / 2` по `x` (глиф в центре колонки шириной `rect.width`; ascent — к правому краю,
как у поворота по часовой) во всех трёх бэкендах; поправить тест femtovg. Локальная проверка только на `cpu_raster.rs` (не влита):
css-writing-modes `float-vlr-003…013`, `float-vrl-002…012`, `margin-collapse-vlr-010/011`, `-vrl-010` из `thick` в `identical`.
Затрагивает пиксели — полный `python graphic_tests/run.py --continue-on-fail` и перегенерация эталонов с вертикальным текстом
(`145-writing-mode`, `1000000-final`) в том же коммите.

## Закрытие

Исправлен `b08924bf5` (BUG-553 срез 62): отображение `(-y + rect.x + rect.width, x + rect.y)` во всех трёх бэкендах. Проверено 2026-10-08 репро из «Как воспроизвести» (без Ahem, глифы Inter, `--screenshot`, dev-release): зелёный текст `x = 70..106` внутри колонки `x = 60..110`. Текст в описании («Причина», «Что делать») — состояние до правки.
