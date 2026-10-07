# BUG-1339 — `word-spacing` / `letter-spacing` измеряются в раскладке, но не рисуются, если слова в одном `DrawText`

**Статус:** OPEN
**Заведён:** 2026-10-06 (P2, WPT-RUN-14 срез 11, `css/CSS2` (backgrounds + borders))
**Область:** paint/layout (`crates/engine/paint/src/display_list/commands.rs:54` — у `DrawText` нет поля интервала; `crates/engine/layout/src/box_tree/inline_wrap.rs:26` `measure_text_w` учитывает `letter_spacing`)

## Симптом

`font: 25px/1 Ahem`, `<div style="letter-spacing:20px">AB</div>`: `offsetWidth` = 53.6 (раскладка учла 20 px), но `--screenshot` — глифы вплотную, как без `letter-spacing`; `word-spacing: 75px` у `<div>1 2</div>` — то же (`getComputedStyle` отдаёт `75px`, в `--dump-display-list` один `DrawText "1 2"`). Если слова разведены разметкой (`<b>A</b> B`), второй `DrawText` получает смещённый `x` (74.28) — там работает.

## Как найдено

WPT-RUN-14 срез 11: `borders/border-001.xht`, `border-003.xht` — эталон `div { font: 25px/1 Ahem; width: 6em; word-spacing: 3em }` с текстом `1 2 3 4 5 6 7 8`, у Lumen пробелы без растяжения. Затрагивает любой текст с `letter-spacing` на реальных страницах (заголовки, кнопки).

## Что делать

Передать `letter-spacing`/`word-spacing` в `DisplayCommand::DrawText` (в обоих растеризаторах) или разбивать текст на сегменты по пробелам в layout.

## Как проверить

`css/CSS2/borders/border-001.xht` — после правки `identical`.

## Срез 14 (2026-10-07, P2, WPT-RUN-14 `css/CSS2` text/linebox/fonts/…)

54 `thick` id в `text/letter-spacing-*`, `word-spacing-*`, `*-applies-to-*` (правило: `letter-spacing`/`word-spacing` в `<style>` теста или эталона, каталог `text`/`linebox`). Проба: `<div style="letter-spacing:10px">XXX</div>` — один `DrawText "XXX"` с рамкой 296 px, глифы вплотную; то же у `word-spacing:40px` и у `<span>`, `<p>`. Раскладка ширину учитывает (`measure_text_w`), растр — нет; `letter-spacing: -0px`-тесты (`letter-spacing-004.xht`, `word-spacing-004.xht`) проходят. Закрытие даёт до 54 id (пересекается с BUG-1372 и BUG-1369).
