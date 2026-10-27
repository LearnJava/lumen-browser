# BUG-1765 — `<style media>` игнорируется каскадом; `matchMedia("bogus").media` — `bogus`

**Статус:** OPEN
**Заведён:** 2026-10-27 (P3, остаток [BUG-1553](BUG-1553-FIXED.md))
**Область:** shell/css (`crates/shell/src/doc_extract.rs` — `walk_style_blocks`, `crates/engine/css-parser/src/parser/media.rs` — `serialize`)

## Симптом

`<style media="print">* { color: red }</style>` красит страницу в красный: атрибут `media` у `<style>` не участвует в каскаде (у `<link>` гейт `link_media_matches` есть). Поэтому `document.styleSheets[0].media.mediaText = "not all"` (теперь пишет атрибут) и `style.setAttribute("media", "not all")` не меняют отрисовку: `css/cssom/medialist-dynamic-001.html` остаётся красным.

Второе: `matchMedia("bogus").media` — `bogus`; Chromium отдаёт `not all` для неизвестного media-type.

## Проба

`lumen --dump-display-list` для `<style media="print">* { color: red }</style><body>text` — `DrawText … #ff0000ff`.

## Что делать

Гейтить `<style>` по `media` при сборке листов (без поломки индексов CSSOM-реестра) и пересобирать каскад при изменении атрибута `media`; в `serialize` сворачивать неизвестный media-type в `not all`.

## Как проверить

`css/cssom/medialist-dynamic-001.html` (reftest).
