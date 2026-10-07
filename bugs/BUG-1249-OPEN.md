# BUG-1249 — Соседние боксы с дробной границей оставляют AA-шов: при `y = 25.72` нижняя граница первого и верхняя второго блока закрашиваются частичным покрытием к

**Статус:** OPEN
**Заведён:** 2026-10-03 (P2, WPT-RUN-14 срез 1, `css/css-flexbox`)
**Область:** paint (`crates/engine/paint/src/cpu_raster.rs` — растеризация прямоугольников с дробными координатами)

## Симптом

Соседние боксы с дробной границей оставляют AA-шов: при `y = 25.72` нижняя граница первого и верхняя второго блока закрашиваются частичным покрытием каждая, и фон родителя (`red`) просвечивает 1-px линией (`rgb(51,102,0)` = 0.8·green + 0.2·red вместо чистого green). Браузеры с pixel-snapping границ бокса (Chrome/Edge) шва не дают. Не flex-специфично: воспроизводится на трёх блоках `height:25px` в `div` с красным фоном после абзаца. WPT-RUN-14-S1: 130 из 710 упавших reftest `css/css-flexbox` различаются с эталоном ТОЛЬКО 1-px линиями (`thin-only`), типичный — `flex-flow-007.html` (5 линий по 100 px).

## Описание

Снимок `--screenshot` страницы

```html
<body style="margin:8px"><p>x</p>
<div style="background:red;width:100px"><div style="background:green;height:25px"></div>
<div style="background:green;height:25px"></div><div style="background:green;height:25px"></div></div></body>
```

на строках y = 50 и 75 (граница двух зелёных блоков; верх контейнера на 25.72) даёт `rgb(51,102,0)` вместо `rgb(0,128,0)`. Красный фон родителя виден сквозь шов.

## Как найдено

WPT-RUN-14 срез 1: 710 упавших reftest `css-flexbox` отрендерены вместе с эталонами (`--screenshot`, 300×250), разность попиксельно; 130 пар отличаются только изолированными 1-px линиями. Эталон, у которого красного фона под зелёным нет, шва не показывает — поэтому проходит бы и с шовом, а тест с красным фоном «нет красного» падает.

## Гипотеза (не проверена)

Привязка рёбер бокса к целым пикселям в растеризаторе (CSS Painting: «pixel snapping»). Править на уровне `DisplayCommand` — единообразно для CPU- и wgpu-пути (`crates/engine/paint/CLAUDE.md`).

## Как проверить

Перерисовать пример выше `--screenshot`; затем `run_corpus.py --prefixes css/css-flexbox` — число reftest `thin-only` (`.tmp/probe/pixdiff.py`, см. `docs/wpt-vendor-notes/css.md` §css-flexbox) должно упасть с 130.

## Дополнение: WPT-RUN-14 срез 11 (2026-10-06, `css/CSS2`: backgrounds + borders)

Ещё два id с тем же швом на дробной границе, не связанных с `<img>`: `borders/border-bottom-width-003.xht`, `border-top-width-003.xht` (`border-bottom-width: 1px` на `y = 25.72`: строки `(71,71,71)` и `(183,183,183)` вместо чёрной линии). Соседний дефект с другой командой — `DrawImage` — заведён отдельно: [BUG-1337](BUG-1337-OPEN.md) (249 id в этих двух каталогах).

## Срез 13 (2026-10-07, P2, WPT-RUN-14 `css/CSS2` tables/positioning/floats/floats-clear/abspos…)

Кандидат: 18 id `thin-only`, эталон — не `<img>` (`abspos/static-inside-inline-001.html`, `-003.html`, `floats-clear/clear-on-child-with-margins-2.html`). Механизм по аналогии (AA-кромка на дробной границе), A/B не делался.
