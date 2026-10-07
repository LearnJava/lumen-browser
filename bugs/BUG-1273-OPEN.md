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

## Дополнение: WPT-RUN-14 срез 9 (2026-10-06, `css/css-text`, часть 1)

59 reftest `css/css-text` падают в wptrunner, но с локальным Ahem (`reftest_pixdiff.py --viewport 800x600 --ahem`) pixel-identical с эталоном: `line-break/` 25, `text-align/` 20, `overflow-wrap/` 12, `hyphens/` 1, `letter-spacing/` 1. Шрифты `mplus-1p-regular.woff` (`i18n/css3-text-line-break-opclns-*`, 158 reftest) и Noto Naskh Arabic (`shaping/`, `boundary-shaping/`) подгружаются тем же `@font-face url()`; влияет ли BUG-1273 на них в wptrunner — не проверялось (проба `document.fonts.load` в testharness даёт `loaded`, но это не снимок).

## Дополнение: WPT-RUN-14 срез 10 (2026-10-06, `css/css-text`, часть 2)

20 reftest второй половины `css-text` падают в wptrunner, но с локальным Ahem pixel-identical с эталоном: `white-space/` 13 (`pre-wrap-leading-spaces-004…010`, `pre-wrap-017`, `hanging-whitespace-003/004`, `white-space-intrinsic-size-003`, …), `word-break/` 4, `text-transform/` 2 (`fullwidth-006/008`), `word-spacing/` 1.

## Срез 14 (2026-10-07, P2, WPT-RUN-14 `css/CSS2` text/linebox/fonts/…): прогон под `wptrunner` нестабилен на холодном старте

Первый прогон `run_corpus.py --prefixes css/CSS2/{text,linebox,fonts,generated-content,lists,bidi-text}` сразу после сборки и обновления манифеста дал 436 зелёных из 1 259 (34.76 %); два повторных прогона на той же сборке — по 506 (40.32 %), побайтно одинаковый вердикт. Разошлись 74 id: 72 из них (`fonts/font-0*`, `linebox/*`, `text/*` — все с `Ahem`) в первом прогоне FAIL, а в повторных PASS, и все 72 — `identical` по `reftest_pixdiff.py` при 800×600, 2 наоборот: `fonts/font-family-013.xht`, `fonts/fonts-013.xht` (в первом прогоне прошли, потому что Ahem не подгрузился и обе стороны отрисовались запасным шрифтом). Причина «холодного» состояния не установлена (гипотеза — первый `@font-face url()` по `localhost:18300/fonts/ahem.css` не успевает до снимка; HTTP-кэш не проверялся). Вывод: один прогон `run_corpus.py` после сборки не воспроизводим на ~6 % срезa с Ahem; числа срезов 1–13 такой проверки (повтор на той же сборке) не проходили.

## Срез 15 (2026-10-07, P2, WPT-RUN-14 `css/CSS2` остальное)

Холодного старта в этом срезе не видно: повторный прогон на чистом каталоге дал 0 расхождений из 1 381 id. Но **28 id `identical` при 800×600 (26 из них и при 1024×720) FAIL под `wptrunner`**, все используют `Ahem`; 9 — с `class="reftest-wait"` и скриптом на `onload` (`box-display/delete-block-in-inlines-*.xht`, `insert-block-in-inlines-*.xht`, `insert-inline-in-blocks-n-inlines-*-002.xht`). Три пробы под `run_corpus.py --prefixes <id>` (`anonymous-boxes-inheritance-001`, `delete-block-in-inlines-end-001`, `insert-block-in-inlines-end-001`) снова FAIL — значит, это не холодный старт, а постоянный разрыв между `--screenshot` из командной строки (Ahem через `LOCALAPPDATA`-подмену из `docs/probe-method.md`) и снимком IPC-исполнителя, где `@font-face url()` не ждётся (см. «Причина» выше). Закрытие BUG-1273 даст до 28 id здесь и 72 в срезе 14.

## Дополнение: WPT-RUN-14 срез 18 (2026-10-07, `css/css-fonts`, `css/WOFF2`)

Масштаб в шрифтовых модулях. Из 1 166 reftest/crashtest-id среза у 566 в тесте или эталоне есть `@font-face … url()`
(`css-fonts` — 268 из 381 reftest/crashtest-id, `WOFF2` — все 298; проверка по тексту). Проба: `@font-face{src:url(/_s18probe/Ahem.ttf)}`
по http, `font:50px/1 AH` — 1 544 зелёных px (ожидается 10 000 для `XXXX`): шрифт не применён. Следствия:

- **`css/WOFF2` — 264 PASS ненадёжны.** Тест (`F` запасным шрифтом) и эталон (`P`) рисуются одним и тем же шрифтом Inter и совпадают
  *случайно* в том смысле, что у обоих нет ни одного загруженного веб-шрифта; 34 FAIL — те, где разметка теста и эталона
  различается. Валидные WOFF2 (`valid-001.woff2` и т. д.) в снимок не попадают. Считать WOFF2 «88,6 %» нельзя до этого бага.
- `css-fonts`: 5 `identical` при 800×600 (`font-size-adjust-006/007/008.xht`, `line-gap-override.html`, `variations/variable-avar2-*`) —
  FAIL под `wptrunner`, зелёные при `--screenshot --ahem`; ещё 218 из 230 падающих reftest `css-fonts` зависят от веб-шрифта
  (`@font-face` в тесте или эталоне, либо `Ahem`), поэтому их причина не локализуема до закрытия этого бага
  (`font-variant-*`, `font-synthesis-*`, `font-size-adjust-*`, `size-adjust`, `ascent-descent-override`, `font-palette-*`,
  `font-kerning-*`: см. кластеры §css-fonts в `docs/wpt-vendor-notes/css.md`).
