# BUG-1372 — Ahem 16 px: у глифа-квадрата `X` верхняя и нижняя строки пикселей — частичное покрытие, у блока того же размера — чистый чёрный

**Статус:** OPEN
**Заведён:** 2026-10-07 (P2, WPT-RUN-14 срез 14, `css/CSS2` (text, linebox, fonts, generated-content, lists, bidi-text))
**Область:** paint/font (`crates/engine/paint/src/cpu_raster.rs` — растр глифа; семейство берётся из системного индекса при `LUMEN_CPU_SYSTEM_FONTS=1`)

## Симптом

`<div style="position:absolute;font:16px/1 Ahem">X</div>` на `y=0`, `--screenshot` с рецептом Ahem (`docs/probe-method.md`), столбец `x=4`:

| `font-size` | строки глифа | значения |
|---|---|---|
| 10 px | `0…9` | все `0` (чёрный) |
| **16 px** | `0…16` (17 строк) | `y=0` — `64` (серый), `y=1…15` — `0`, `y=16` — `192` |
| 20 px | `0…19` | все `0` |
| 25 / 30 / 40 px | полные | все `0` |

Ahem `X` — ровный квадрат `1em × 1em`: при 16 px это 16 чистых строк. Lumen рисует 17: у верха 75 % чёрного, у низа 25 %. Эталон тех же тестов строит ту же фигуру из `div` с `background: black; height: 1em` — чистые 16 строк. Разница — две строки по 32–48 px: `thin-only`.

## Как найдено

WPT-RUN-14 срез 14: 51 id `thin-only` без `<img>` в эталоне (`text/white-space-processing-*`, `word-spacing-*`, `fonts/font-size-12*`); у 50 из 51 шрифт — `font: 16px/1em Ahem`. A/B: тест при `p{margin:0;height:18px;line-height:18px}` (целая `y` текста) остаётся `thin-only` у 50 из 51 — значит, дело не в дробной `y` из `<p>` (BUG-1249). Не проверено: воспроизводится ли у живого окна (wgpu) и без `LUMEN_CPU_SYSTEM_FONTS=1` (без него Ahem в `--screenshot` не находится вовсе).

## Что делать

Выяснить, чем 16 px отличается от 10/20: вертикальные метрики (`ascent`/`descent` Ahem 0.8/0.2 em → 12.8 + 3.2 = 16, округление при 16 даёт 17 строк?), и привести растр глифа к целой сетке, когда `ascent`/`descent` дают целую высоту.

## Как проверить

`css/CSS2/text/white-space-processing-002.xht` — `reftest_pixdiff.py --viewport 800x600 --ahem` даёт `identical`.
