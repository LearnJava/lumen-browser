# BUG-1263 — `inline-block` и `<br>` не текут вертикально внутри блока с вертикальным `writing-mode`

**Статус:** OPEN
**Заведён:** 2026-10-04 (P1, FLEX-VWM)
**Область:** layout (`crates/engine/layout/src/vertical.rs`, `box_tree/layout_dispatch.rs` — ветка `InlineBlockRow` в вертикальном контексте)

## Симптом

В блоке с `writing-mode: vertical-rl|vertical-lr` строка `inline-block`-ов (`InlineBlockRow`) раскладывается как в горизонтальном режиме: боксы идут слева направо, а не сверху вниз, `<br>` не начинает новую колонку.
Вертикально течёт только `InlineRun` (`lay_out_vertical_inline_run`).

```html
<div style="writing-mode:vertical-rl;line-height:0">
  <span style="display:inline-block;width:15px;height:45px;background:orange"></span><br>
  <span style="display:inline-block;width:15px;height:45px;background:grey"></span>
</div>
```

`--dump-layout`: оба `inline-block` имеют `x = 2`, строка `InlineBlockRow` шириной во весь контейнер; ожидается колонка из двух блоков друг под другом (оранжевый над серым), а `<br>` — переход в следующую колонку.

## Описание

`vertical.rs` в шапке прямо говорит, что «FormControl and other box kinds inside a vertical context still fall through to horizontal layout»; `InlineBlockRow` — один из таких видов.
Ломает WPT `css/css-flexbox/css-flexbox-row.html` / `css-flexbox-row-reverse.html` (элементы flex-контейнера с `writing-mode: vertical-rl` содержат ровно такие столбцы цветных `inline-block`-ов),
`flex-item-vertical-align.html` и часть `flexbox-writing-mode-*`, где элемент — вертикальный блок с `inline-block`-ами.

## Как найдено

FLEX-VWM: после того как вертикальный flex-контейнер стал раскладываться как flex, `css-flexbox-row.html` дал контейнер на всю ширину и два цветных блока вместо четырёх.

## Как проверить

Проба выше; `tests/wpt/css/css-flexbox/css-flexbox-row.html` — совпадение с эталоном (`reftest_pixdiff.py`).
