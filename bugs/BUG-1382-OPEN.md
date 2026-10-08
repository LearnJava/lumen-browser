# BUG-1382 — горизонтальные поля inline-бокса теряются, если у бокса нет фона, рамки и padding

**Статус:** OPEN
**Заведён:** 2026-10-07 (P2, WPT-RUN-14 срез 15, `css/CSS2` (остальные каталоги: selectors, css1, syntax, box-display, visufx, visudet, …))
**Область:** layout (`crates/engine/layout/src/box_tree/inline_build.rs:1020-1028` — поле `margin-right` inline-бокса без фона и рамки теряется при слиянии сегментов)

## Симптом

`--dump-display-list`, `div{font:20px/1}`:

| разметка | позиции `DrawText` | ожидается |
|---|---|---|
| `<span style="margin-right:40px">x</span>y` | один `"xy"` на `x=0` | `x` на 0, `y` на ~50 |
| `<span style="margin-right:40px;background:red">x</span>y` | `"x"` на 0, `"y"` на **48** | верно |
| `<span style="padding-right:40px">x</span>y` | `"x"`, `"y"` на 48 | верно |
| `a<span style="margin-left:40px">x</span>y` | `"a"` на 0, `"xy"` на **47.1** | верно |
| `a<span style="margin-right:40px">x</span>y` | `"a"` на 0, `"xy"` на **7.1** | `y` на ~47 |
| `a<span style="margin-right:40px">x</span> y` | `"x y"` на 7.1 | `y` после 40px поля |

`--dump-layout`: у `<span margin-right>` сегменты `x` и `y` склеены в один фрагмент `xy` (`seg[0] "x"`, `seg[1] "y"`, `frag[0] "xy"`). `margin-left` учитывается, `margin-right` нет — пока у бокса нет декорации, которая принуждает отдельный сегмент (фон, рамка, padding). Расчёт `post` (`inline_build.rs:1024-1026`) включает `margin_right`, но слияние соседних сегментов идёт раньше и не видит «пустой» декорации.

## Как найдено

WPT-RUN-14 срез 15: `css/CSS2/css1/c5502-imrgn-r-002…006`, `c5502-mrgn-r-000`, `c5504-imrgn-l-*` — тест ставит `margin-right:4em` на `<span>` и ждёт, что красный фон «съехал» из-под зелёного: 6 id в таблице кластеров (+ до 18 в `css1/c55xx-*` по тому же шаблону без отдельной пробы).

## Что делать

При слиянии соседних inline-сегментов учитывать ненулевой `margin-right`/`margin-left` как разрыв, как уже делается для `padding`/`border`.

## Как проверить

`css/CSS2/css1/c5502-imrgn-r-002.xht`, `c5504-imrgn-l-003.xht`; проба — страница из таблицы выше.
