# BUG-1530 — Пустой `<span>` с горизонтальными `padding`/`border`/`margin` не занимает места в строке и не рисует фон

**Статус:** FIXED 2026-10-27 (P3)
**Заведён:** 2026-10-08 (P2, WPT-RUN-14 срез 24, `css/filter-effects` + `css-inline` + `css-tables` + `css-align`)
**Область:** layout (`crates/engine/layout/src/box_tree/inline_build.rs` — пустой inline-бокс с `padding`/`border`/`margin`)

## Симптом

`x<span style="padding:0 10px;background:red"></span>y` рисует `x` и `y` вплотную без красной полосы (ожидается пробел 20 px и фон); то же с `border-left:10px solid`, `margin-left:10px`. Если внутрь положить любой символ (в т. ч. ZWSP), полоса и сдвиг появляются. Пустой inline-бокс с ненулевыми полями обязан порождать пустой inline-бокс (CSS 2.1 §9.4.2, CSS Inline 3 «phantom line boxes»: строка не фантомная, если есть поля/рамки/отступы по инлайн-оси). 8 reftest.

## Проба

Проба (`--screenshot`, `font:20px/20px Ahem`, красные пиксели в строке `y=10`):

| разметка | красных пикселей | ожидается |
|---|---|---|
| `x<span style="padding:0 10px;background:red"></span>y` | **0** | 20 |
| `x<span style="padding:0 10px;background:red">z</span>y` | 28+ (рисуется) | рисуется |
| `x<span style="margin-left:10px"></span>y`, `x<span style="border-left:10px solid red"></span>y` | `y` не сдвинут | сдвиг 10 |


## Как найдено

WPT-RUN-14 срез 24: `css-inline/empty-span-size-001/002`, `model/phantom-line-boxes-001…006`.

## Что делать

Не отбрасывать пустой inline-бокс, если у него ненулевые padding/border/margin по инлайн-оси: оставлять рамку/фон и ширину; нулевые — оставлять поведение.

## Как проверить

`css/css-inline/model/phantom-line-boxes-001.html`, `empty-span-size-001.html`.


## Что сделано

- `collect_inline_segments` (`box_tree/inline_build.rs`): если у inline-элемента не нашлось ни одного сегмента (нет текста, `<br>`, детей-блоков) и хотя бы одна из шести составляющих (`margin`/`border`/`padding` слева и справа) не нулевая, в строку кладётся пустой краевой сегмент (`text: ""`, `is_element_box`, `pre_space`/`post_space`) — как для кусков inline-бокса, разрезанного блоком. Нулевые поля поведение не меняют. Условие по отдельным составляющим, а не по сумме: `margin-left:-1px` и `padding-left:1px; margin-left:-1px` тоже делают строку нефантомной (CSS Inline 3 «invisible line boxes»).
- WPT `css-inline/model/phantom-line-boxes-001…006`: 6 из 6 попиксельно. `empty-span-size-001/002` остаются красными — там `outline` у пустого span без полей, `<input>`, `<li>` и `scrollIntoView` под `reftest-wait`, к полям inline-оси это не относится.
- Тесты: `box_tree::tests::block_in_inline_slice::empty_inline_box_*`. Display list 191 страницы корпуса (`graphic_tests`, `samples`) до/после — байт-в-байт.

