# BUG-1333 — `word-spacing` в процентах (и `calc()` с `%`) не применяется

**Статус:** OPEN
**Заведён:** 2026-10-06 (P2, WPT-RUN-14 срез 10, `css/css-text`, вторая половина)
**Область:** layout (`crates/engine/layout/src/style/apply/text.rs` — разбор `word-spacing`; `getComputedStyle` отдаёт `0px`)

## Симптом

`font: 20px monospace`, `<i>A</i> <i>B</i>`, `x` слова `B`:

| `word-spacing` | получено | ожидается |
|---|---|---|
| `0` | 19 | 19 |
| `20px`, `1em` | 39 | 39 |
| `100%` | **19** | 39 (процент — от ширины U+0020 шрифта) |
| `calc(25% + 0px)` → `getComputedStyle` | `0px` | `2.75px` |
| `-5px` | 14 | 14 |

## Как найдено

WPT-RUN-14 срез 10: `word-spacing/word-spacing-001.html`, `-002.html`, `-003.html`, `word-spacing-percent-001.html` (reftest `thick`); `word-spacing-negative-value-001.html` pixel-identical с локальным Ahem (BUG-1273).

## Что делать

Принять `<percentage>` в `word-spacing` (и внутри `calc()`), хранить до раскладки и резолвить от ширины пробела текущего шрифта.

## Как проверить

`css/css-text/word-spacing/word-spacing-001.html`.
