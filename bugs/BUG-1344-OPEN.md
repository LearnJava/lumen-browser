# BUG-1344 — В сокращении `background` длины в `in`/`cm`/`mm`/`pt`/`pc`/`ex`/`ch`/`calc()` сбрасывают позицию в `0% 0%`

**Статус:** OPEN
**Заведён:** 2026-10-06 (P2, WPT-RUN-14 срез 11, `css/CSS2` (backgrounds + borders))
**Область:** css-parser/layout (`crates/engine/layout/src/style/parse/` — разбор `<position>` внутри `background`; longhand `background-position` те же единицы разбирает)

## Симптом

Позиция из `background: url(x.png) no-repeat <v> <v>` (`--dump-display-list`, `pos=`):

| `<v>` | сокращение | `background-position: <v> <v>` |
|---|---|---|
| `10px`, `1em`, `1rem`, `1vw`, `10%` | верно | верно |
| `0.5in`, `1cm`, `1mm`, `1pt`, `1pc`, `1q`, `1ex`, `1ch`, `calc(5px + 1em)` | **`0% 0%`** | верно (`48px 48px`) |
| `48px 0.5in` | `48px 50%` | `48px 48px` |

## Как найдено

WPT-RUN-14 срез 11: `backgrounds/background-329.xht` (`background: black url(…) repeat-x 0.5in 0.5in`) — 1 id; на реальных сайтах — `background: … 10pt 5mm`.

## Что делать

Единый разбор длины для позиции в сокращении и в longhand (`parse_line_width`-подобный общий путь).

## Как проверить

`css/CSS2/backgrounds/background-329.xht`.
