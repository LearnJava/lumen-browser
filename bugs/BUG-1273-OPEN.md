# BUG-1273 — снимок `--screenshot` и IPC `Screenshot` (reftest-исполнитель WPT) не дожидаются `@font-face url()`: Ahem не применяется ни в одном reftest

**Статус:** OPEN
**Заведён:** 2026-10-05 (P2, WPT-RUN-14 срез 2 — `css/css-writing-modes`)
**Область:** shell (`crates/shell/src/dump_mode.rs::render_source_to_png`, `page_pipeline.rs` — `pending_web_fonts`)

## Симптом

Страница с `<link rel=stylesheet href="/fonts/ahem.css">` и `font: 50px/1 Ahem`, отданная по http (wptserve /
`tests/wpt/serve_wpt_like.py`), в `lumen --screenshot` рисуется запасным шрифтом: `XXXX` занимает 112 px вместо 200,
глифы — контуры Inter, а не сплошные квадраты Ahem.

`render_source_to_png` — общая точка `--screenshot`, IPC `Screenshot` (им ходит `LumenRefTestExecutor`,
`tools/wptrunner/wptrunner/executors/executorlumen.py:937`) и BiDi `Screenshot`. Поэтому **ни один reftest WPT не
рисуется с Ahem**, хотя `tests/wpt/fonts/ahem.css` отдаётся сервером, а `Ahem.ttf` вендорен (TEST-5).

## Причина

`load_font_faces` (`subresources.rs:286`) регистрирует синхронно только `local()`; `url()` уходит в
`pending_web_fonts` для фоновой загрузки и FOUT-relayout по `LoadEvent::FontLoaded` (PH3-19). У однократного
headless-пути нет цикла событий: `render_source_to_png` рисует первый layout и выбрасывает `pending_web_fonts`
(`page_pipeline.rs:134` переносит их только в живое окно). TEST-5 проверял Ahem в живом MCP-окне, а не в этом пути.

## Масштаб

В `css-writing-modes` 534 из 1081 reftest используют Ahem (тест или эталон); из них PASS — 33 (6 %), у тех, что без
Ahem — 84 из 547 (15 %). Во всём `css` Ahem упоминают 3 643 из 24 552 reftest-файлов. Тест и эталон сравниваются
одинаково «не тем» шрифтом, поэтому часть совпадает случайно, а часть расходится из-за разной ширины запасного
шрифта у разной разметки (`docs/probe-method.md` §Reftest-A/B).

## Что делать

В headless-пути (`render_source_to_png`) дождаться `pending_web_fonts` синхронно (с бюджетом времени, как
`SCREENSHOT_RAF_TURNS` для rAF), зарегистрировать их в `font_registry` и измерителе и пересчитать layout до снимка.
Это и есть поведение WPT: снимок reftest-а делается после `document.fonts.ready`.

## Как проверить

Проба выше (`XXXX` = 200 px сплошного цвета). Затем `run_corpus.py --prefixes css/css-writing-modes` — доля PASS у
Ahem-reftest-ов не должна быть ниже, чем у остальных.
