# BUG-1711 — `position: relative` на обычном inline-боксе (`<span>` с текстом) не сдвигает его: `top`/`left` игнорируются

**Статус:** OPEN
**Заведён:** 2026-10-26 (P3, при BUG-1522)
**Область:** layout (inline-раскладка: `crates/engine/layout/src/box_tree/layout_dispatch.rs::relative_offset` вызывается для блоков, flex/grid-элементов и inline-block, но не для текстового inline-бокса)

## Симптом

`<p>left<span style="position:relative;top:30px;text-decoration:underline">XXXX</span></p>` — `--dump-display-list` и `--screenshot` дают текст и подчёркивание на строке абзаца: смещение `top:30px` не применено (DrawText `y=0.86`, FillRect подчёркивания `y=15.26`, как без `position`). Сдвиг по `left` — по тому же пути.

## Проба

`--viewport 400x200 --dump-display-list` на странице выше; то же при `top:0` (нет смещения) — вывод идентичен, т.е. `top:30px` не оказывает эффекта. Отличается от BUG-1472 (там блок-потомок внутри relative-inline); здесь внутри только текст.

## Как найдено

BUG-1522: WPT `css-text-decor/text-underline-offset-002.html` двигает `<span id=text>` через `position:relative; top:10px` (эталон — `top:21px`); у нас оба сдвига пропадают, и тест остаётся `thin-only` при верном подчёркивании.

## Что делать

Применять `relative_offset` к фрагментам inline-бокса (`InlineFrag`) после раскладки строки: сдвиг не влияет на поток (CSS 2.1 §9.4.3), вложенные inline-боксы и их декорации/фон/рамка сдвигаются вместе с ним.

## Как проверить

Страница из «Симптома»: DrawText и подчёркивание смещены на 30 px вниз; `css/css-text-decor/text-underline-offset-002.html` — `identical`.
