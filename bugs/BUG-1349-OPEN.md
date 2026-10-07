# BUG-1349 — Отрицательные `width`, `height`, `min-*`, `max-*` и `padding*` принимаются, а должны отбрасываться (CSS 2.1 §8.4, §10.2, §10.4, §10.5, §10.7)

**Статус:** OPEN
**Заведён:** 2026-10-07 (P2, WPT-RUN-14 срез 12, `css/CSS2` (normal-flow + margin-padding-clear))
**Область:** layout/css-parser (`crates/engine/layout/src/style/apply/`, `style/parse/` — `width`/`height`/`min-*`/`max-*`/`padding*`)

## Симптом

`--dump-layout`, `font: 20px/1 Ahem`: `<div style="width:-1px">` → `w=-1.00`, бокс 0 px (при отброшенном объявлении — `auto`, вся строка); `height:-5px` → `h=-5.00`; `padding-top:-1em` → `p=(-1.00em, …)`, `rect` высотой `-5` вместо 10; `max-width:-1%`, `max-height:-1em` принимаются и схлопывают бокс (`max-w=-1.00em`, `rect` ширины 0). Недопустимое объявление отбрасывается целиком, значение остаётся прежним.

## Как найдено

WPT-RUN-14 срез 12: 16 id с «minimum minus one value» в названии (`normal-flow/{height-089,max-height-067,max-height-078,max-width-089,…}`, `margin-padding-clear/padding-{top,right,bottom,left}-067|078`). **A/B** одним бинарём (копии пар «тест + эталон» без объявлений с отрицательной длиной у этих свойств): 12 thick из 12 → `identical`/`thin-only`, новых провалов нет; ещё 4 id (`padding-{top,right,bottom,left}-089`, `-1%`) уже `thin-only` до правки и в A/B не входили.

## Что делать

При разборе перечисленных свойств отвергать отрицательную длину/процент (`calc()` с отрицательным итогом допустим и зажимается при вычислении — другой путь). Тот же отказ — в CSSOM-валидаторе (`el.style.width = "-1px"` не меняет значение).

## Как проверить

`css/CSS2/margin-padding-clear/padding-top-067.xht`, `css/CSS2/normal-flow/max-width-089.xht` — `reftest_pixdiff.py --viewport 800x600 --ahem` даёт `identical`.
