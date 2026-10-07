# BUG-1409 — `font-size: xx-small…xxx-large`, `larger`, `smaller` не применяются; отрицательный `calc()`-размер не зажимается в 0

**Статус:** OPEN
**Заведён:** 2026-10-07 (P2, WPT-RUN-14 срез 18, `css/css-fonts` + `css/css-masking` + `css/WOFF2`)
**Область:** layout (`crates/engine/layout/src/style/parse/font_size.rs::resolve_font_size`, `:130`)

## Симптом

`--dump-layout` и `getComputedStyle`, `body{margin:0}`:

| `font-size` | `getComputedStyle().fontSize` | высота строки (`InlineRun`) |
|---|---|---|
| `xx-small` … `xx-large` (7 значений), `larger`, `smaller` | **`16px`** у всех | 17,72 px у всех |
| `medium` | `16px` | 17,72 |
| `200%` | `32px` | 35,44 |
| `48px` | `48px` | 53,16 |
| `calc(-10px)` | **`-10px`** | — (ожидается `0px`) |
| `-5px` | **`-5px`** | — (невалидно, должно наследоваться) |
| `font: xx-large serif` | `16px` | 17,72 |

Ожидаемые px (CSS Fonts L4 §2.5): `xx-small` 9, `x-small` 10, `small` 13, `medium` 16, `large` 18, `x-large` 24,
`xx-large` 32, `xxx-large` 48; `larger`/`smaller` — по таблице от размера родителя. Через `<font size=7>` размер
48 px выходит (presentational hint, `table_grid_presentational.rs:891`) — значит, в раскладке таблица размеров есть,
не хватает разбора слов в CSS.

## Как найдено

WPT-RUN-14 срез 18: `css-fonts/font-size-xxx-large.html` (reftest: `font-size: xxx-large` против `<font size=7>` —
рисуется 17,72 px против 48), `parsing/font-size-computed.html` (`<font size=2…7>` читается `40px`),
`parsing/font-size-invalid.html`, `font-size-relative-across-calc-ff-bug-001.html` (`-1599px` вместо `0px`),
`font-shorthand-serialization-font-stretch.html`.

## Что делать

В `resolve_font_size` разрешать absolute-size по таблице и relative-size от `parent_fs`; зажимать
`calc()`-результат в `[0, ∞)`; отбрасывать отрицательные литералы.

## Как проверить

`css/css-fonts/font-size-xxx-large.html`, `parsing/font-size-computed.html`, `parsing/font-size-invalid.html`.
