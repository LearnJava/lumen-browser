# BUG-1523 — Подчёркивание предка распространяется на float, `position:absolute` и содержимое `inline-block`/`inline-flex` (CSS Text Decoration 3 §2.2 это запрещает)

**Статус:** OPEN
**Заведён:** 2026-10-08 (P2, WPT-RUN-14 срез 23, `css/css-text-decor` + `css-gaps` + `css-shapes`)
**Область:** layout/paint (`crates/engine/layout/src/box_tree/` — распространение `text-decoration` на потомков; `crates/engine/paint/src/display_list/svg_text_decoration.rs::push_text_decoration`)

## Симптом

Декорация (`text-decoration-line` предка) рисуется под текстом float'ов, абсолютно позиционированных потомков и атомарных inline (`inline-block`, `inline-flex`), хотя по §2.2 она не распространяется на «floating and absolutely positioned descendants, nor to the contents of atomic inline-level descendants such as inline blocks and inline tables». `inline-table` работает верно (0 пикселей).

## Проба

Проба (`--screenshot`, `font:20px/30px Arial`, `<span style="text-decoration:underline;text-decoration-color:red">…</span>`; число красных пикселей = длина подчёркивания; чёрный текст):

| содержимое span | красных пикселей | ожидается |
|---|---|---|
| `abc` | 32 | 32 |
| `<span style="display:inline-block">abc</span>` | **32** | 0 |
| `<span style="display:inline-flex">abc</span>` | **32** | 0 |
| `<span style="float:left">abc</span>x` | **42** | 10 (только `x`) |
| `<span style="position:absolute">abc</span>x` | **42** | 10 |
| `<span style="display:inline-table">abc</span>` | 0 | 0 |

## Как найдено

WPT-RUN-14 срез 23: `text-decoration-propagation-02.html` (inline-flex), `-03`, `-04`, `-display-contents.html`, `-display-contents-002`, `-dynamic-001`, `text-decoration-subelements-001/002` — 8 reftest (5 `thick`, 2 `thin-only`, 1 `no-match-ref`).

## Что делать

Не передавать декорацию через границу float/abspos/atomic inline при построении списка декораций фрагмента; декорации `display:contents` потомков — по §2.2 передаются родителю.

## Как проверить

Таблица выше; `css/css-text-decor/text-decoration-propagation-02.html`, `text-decoration-propagation-03.html`.
