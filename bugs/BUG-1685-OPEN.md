# BUG-1685 — `getComputedStyle` для якорного позиционирования: используемые значения вставок, `ident()`, `anchor()` внутри `calc()`

**Статус:** OPEN
**Заведён:** 2026-10-22 (P3, остаток [BUG-1478](BUG-1478-FIXED.md))
**Область:** layout/css-parser (`crates/engine/layout/src/selector_query.rs` — вставки и размеры якорного элемента; `style/parse/box_sides.rs` — `ident()`, `anchor()` в `calc()`)

## Симптом

После BUG-1478 `anchor-name`/`position-anchor`/`anchor-scope`/`position-area`/`position-visibility` корректно попадают в `getComputedStyle`. Не работает остальное (прогон WPT `css/css-anchor-position`, 2026-10-22):

| тест | подтесты | у нас | ожидается |
|---|---|---|---|
| `anchor-getComputedStyle-001.html` | Basic case; Mixed writing modes | `left` = `-210px` / `-60px` | `100px` |
| `anchor-getComputedStyle-002.html` | фрагментированный containing block (multicol / inline) | `182.48px` / `""` | `25px` / `20px` |
| `anchor-getComputedStyle-003.html` | absolutize `@try` | `999999px` / `0px` | `100px` / `200px` |
| `anchor-getComputedStyle-004.html` | `auto` при `position-area` | `80px` | `0px` |
| `position-area-anchor-getComputedStyle.html` | 4 подтеста | `125px` | `0px` |
| `anchor-ident-function.html` | `ident("--x" calc(42 * sign(...)))` в `anchor-name`/`anchor-scope`/`position-anchor`; `calc(anchor(--a1 left)*4)` | `none` / `--a1` / `0px` | `--myident42` и т.д. |
| `position-area-parsing.html` | 1 800 из 2 125 | `style.setProperty` не принимает | значение принято и сериализовано |
| `anchor-parse-valid.html`, `anchor-size-parse-valid.html`, `property-interpolations.html` | ~1 200 + ~1 800 + ~290 | `anchor()` внутри `calc()` отклонён | принято (GAP-ANCHORCSSOM, [BUG-563](BUG-563-OPEN.md)) |

## Как проверить

`css/css-anchor-position/anchor-getComputedStyle-001.html` … `-004`, `position-area-anchor-getComputedStyle.html`, `anchor-ident-function.html` (`tests/wpt/run_report.py --all --root css/css-anchor-position`).
