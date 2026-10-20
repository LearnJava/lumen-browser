# BUG-1420 — `<object data="x.svg">`/`<embed src="x.svg">` рисуют SVG в натуральном размере со сдвигом 8 px, без масштабирования под бокс

**Статус:** FIXED 2026-10-20 (P3)
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

## Исправление

Причина была не в layout, а в склейке фреймов. Для `<object>`/`<embed>` с SVG шелл держит под-документ (`FrameHandle`, нужен
`contentDocument`/`getSVGDocument()`), а рисует SVG image-конвейер — `DrawImage` по боксу элемента с `object-fit`/`object-position`.
`splice_one_frame` (`crates/shell/src/frames.rs`) ищет заглушку `<iframe>` как `DrawImage` с тем же `src` и тем же прямоугольником —
и принимал за заглушку саму картинку SVG: на её место вклеивался display list под-документа (белый фон + SVG по полям `body`
в натуральном размере, (8,8)). `FrameHandle` получил `is_svg_embed`; `splice_frame_content`, `splice_children_of` и пропуск
`DrawImage` вложенных хозяев в `sync_frame_viewports` такие хэндлы обходят (`FrameHandle::paints_in_host`).

Проверка: `--dump-display-list` для `<object>`/`<embed>` (в т.ч. `object-fit: contain`) — один `DrawImage (0,0,48,32)`;
скриншоты `object-fit-contain-svg-001o/001e` побайтно совпадают с `…001i` (`<img>`, раньше PASS); `object-fit-none-svg-002o`,
`scale-down-svg-003o` совпадают с эталоном побайтно. Тест `splice_frame_content_leaves_svg_embed_image_alone`.
`wptrunner` на 63 id не гонялся.

Самостоятельный `.svg` (поле 8 px у документа верхнего уровня) в этой правке не менялся — вынесен в [BUG-1659](BUG-1659-OPEN.md).
