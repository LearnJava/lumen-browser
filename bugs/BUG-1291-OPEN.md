# BUG-1291 — `position: relative` со `top`/`bottom` не сдвигает `inline-block` и поля формы по вертикали (`left` работает)

**Статус:** OPEN
**Заведён:** 2026-10-06 (P2, WPT-RUN-14 срез 5, `css/css-transforms`)
**Область:** layout (`crates/engine/layout/src/box_tree/` — относительное смещение боксов внутри `InlineBlockRow`)

## Симптом

`--dump-layout`, `<span style="display:inline-block;width:60px;height:20px;position:relative;left:100px;top:50px">`:

| вариант | получено | ожидается |
|---|---|---|
| одиночный `inline-block`, `left:100px; top:50px` | rect (100, 0) | (100, 50) |
| он же вторым в строке (`left:100px; top:50px`) | x = 160 (60 + 100), y = 0 | (160, 50) |
| `<input style="position:relative;top:10px">` внутри `div` | y не меняется | +10 |
| `<input style="position:relative;left:-10px">` | сдвиг по x есть | — |
| `<div style="display:inline-block;position:relative;top:50px">` | y не меняется | +50 |

Горизонтальное смещение применяется, вертикальное — нет; у `display:block` обе оси работают
(связанный, но другой дефект — [BUG-1240](BUG-1240-OPEN.md): смещение утекает в поток у блоков).

## Как найдено

WPT-RUN-14 срез 5: 19 упавших reftest `transform-input-*` (`css/css-transforms`). Тест сдвигает `<input>` через
`transform: translateY(±10px)`, эталон — через `position: relative; top: ±10px`. Транcформ у Lumen верен (снимок теста: строки
10…111), эталон не получает вертикального сдвига (строки 10…101) — расходится сам эталон. Класс шире `css-transforms`:
любой reftest, использующий `position: relative; top` на строчном блоке как «эталонный» способ сместить элемент.

## Что делать

Найти место, где `shift_tree` (относительное смещение) пропускается для боксов в `InlineBlockRow`/строчных вставках по оси y;
учесть `bottom` и `top`+`bottom`. Проверить `FormControl`, `<button>`, `<select>`, `<img>` (строчный replaced).

## Как проверить

`css/css-transforms/transform-input-0{01…19}.html` (wptrunner `--prefixes css/css-transforms`) и `tests/wpt/reftest_pixdiff.py`
на них; плюс `css/CSS2/positioning/relpos-*`, не прогонялись.
