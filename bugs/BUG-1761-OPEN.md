# BUG-1761 — ширина пробела между inline-кусками и вокруг `<img>` не совпадает с шириной пробела в тексте

**Статус:** OPEN
**Заведён:** 2026-10-27 (P3, остаток [BUG-1545](BUG-1545-FIXED.md))
**Область:** layout (`crates/engine/layout/src/box_tree/inline_wrap.rs` — `space_w`, `img_space`; `inline_build.rs` — пробельный текстовый узел после `<img>`)

## Симптом

Проба (`--screenshot`, `LUMEN_CPU_SYSTEM_FONTS=1` + Ahem, `font: 20px/1 Ahem`, где пробел Ahem — 20px):

| разметка | у нас | ожидается |
|---|---|---|
| `<div style="font:16px">…<span style="font:20px Ahem">X <span>X</span> <span>X</span></span></div>` | X на `0, 36, 72` | `0, 40, 80` |
| `<img 60px> <span>X</span>` (и `<b>`, и перенос строки вместо пробела) | X на `60` | `80` |
| `<span>X</span> <img> <span>X</span>` | картинка на `25` | `40` |
| `<img> <img> X` | вторая картинка на `65` | `80` |
| `<img> X` (пробел внутри текстового узла) | X на `80` | `80` — верно |

Три разных причины в одном месте:
1. `space_w` в `wrap_inline_run` меряется по `container_font_size` блока и семейству первого сегмента, а не по шрифту сегмента, к которому пробел относится (отсюда 16 вместо 20 при блоке 16px и Ahem 20px внутри).
2. `img_space` (пробел перед картинкой) использует тот же `space_w` через `char_width_with_families` — пробел Ahem там 5px, у шейпера — 20px.
3. Пробельный узел между `<img>` и следующим элементом (не текстом) пропадает: зазора нет.

## Как найдено

BUG-1545: `css/css-inline/baseline-shift/baseline-shift-{top,bottom,center,length-percentage}.html` — после того, как `baseline-shift` стал сдвигать HTML-текст, вертикальные позиции совпали с эталоном, горизонтальные — на 20/24/28px левее.

## Что делать

Мерить пробел шрифтом того сегмента, чьим пробелом он является (как это делает `measure_text_w_styled` для пробелов внутри текста), и для зазора вокруг `<img>`; разобраться, почему пробельный узел после `<img>` не превращается в зазор перед следующим элементом (`inline_build.rs`, ветка `NodeData::Text` — пробел дописывается к `out.last_mut()`). Пиксели движутся — нужен полный `graphic_tests/run.py`.

## Как проверить

`css/css-inline/baseline-shift/baseline-shift-length-percentage.html` (и `-top`, `-bottom`, `-center`).
