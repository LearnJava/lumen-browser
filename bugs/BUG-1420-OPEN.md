# BUG-1420 — `<object data="x.svg">`/`<embed src="x.svg">` рисуют SVG в натуральном размере со сдвигом 8 px, без масштабирования под бокс

**Статус:** OPEN
**Заведён:** 2026-10-08 (P2, WPT-RUN-14 срез 19, `css/css-images + css/css-values + css/css-color`)
**Область:** layout/paint (вложенный документ `<object>`/`<embed>` с SVG; standalone `.svg`)

## Симптом

SVG `full.svg` (`width=16 height=8 viewBox="0 0 16 8" preserveAspectRatio="none"`, `<rect width=16 height=8 fill=green>`),
viewport 200×100, число зелёных (0,128,0) пикселей и их рамка:

| разметка | зелёных | рамка |
|---|---|---|
| `<img src=full.svg style="width:48px;height:32px">` | 1536 | (0,0)–(47,31) |
| `<object data=full.svg style="width:48px;height:32px">` | **128** | **(8,8)–(23,15)** |
| `<embed src=full.svg style="width:48px;height:32px">` | 128 | (8,8)–(23,15) |
| `<object data=full.svg width=48 height=32>` | 128 | (8,8)–(23,15) |
| то же `style="…;object-fit:contain"` | 128 | (8,8)–(23,15) |
| `<img … object-fit:contain>` (контроль) | 1152 | (0,4)–(47,27) |
| `<object data=full.svg>` без размеров | 0 | |

`--dump-display-list` для `<object>`: `PushClipRect (0,0,48,32)` → `FillRect (0,0,48,32) #ffffffff` →
`PushClipRect (8,8,16,8)` → `FillRect (8,8,16,8) #008000ff`: вложенный документ получает белую подложку и содержимое SVG
в полях `body` (8 px) натурального размера, а не растянутое по `viewBox` под бокс. `--dump-display-list` самого `full.svg`:
`PushClipRect (8,8,16,8)` — те же 8 px у самостоятельного SVG-документа.

## Как найдено

WPT-RUN-14 срез 19: `css-images/object-fit-{contain,cover,fill,none,scale-down}-svg-*[eo].html` и
`object-position-svg-*[eo].html` — 63 `thick` (`e` — `<embed>`, `o` — `<object>`; `i` — `<img>` и `p` — `<picture>` — 64 PASS).
Эталоны рисуют те же картинки `<img>`-ом.

## Что делать

Для `<object>`/`<embed>`, чьё содержимое — SVG-документ: вписывать корневой `<svg>` в бокс ресурса по
`viewBox`/`preserveAspectRatio` (как у replaced-элемента), не прибавлять `body`-поля, применять `object-fit`/`object-position`
к самому боксу. Правка двигает пиксели — полный `graphic_tests/run.py --continue-on-fail` и новые эталоны в том же коммите.

## Как проверить

Страница из таблицы; `css/css-images/object-fit-contain-svg-001o.html`, `…-001e.html`.
