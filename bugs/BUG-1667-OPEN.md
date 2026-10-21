# BUG-1667 — `getComputedStyle(el, '::marker')`: маркер без бокса и `::marker` не-`list-item` отдают пустую строку

**Статус:** OPEN
**Заведён:** 2026-10-21 (P3, остаток [BUG-1446](BUG-1446-FIXED.md))
**Область:** layout/js (`crates/engine/layout/src/lib.rs::collect_pseudo_computed_styles_box`, `marker_style_map`; `crates/engine/layout/src/box_tree/inline_build.rs::inject_marker`)

## Симптом

После [BUG-1446](BUG-1446-FIXED.md) стиль `::marker` публикуется только для существующего `BoxKind::Marker`. Из-за этого:

1. `li { list-style: none }` или `li::marker { content: none }` — записи нет, `getComputedStyle(li, '::marker').content` = `""`; Chrome отдаёт `none`, а `width`/`height` — `auto` (`marker-computed-size.html`, «`::marker` with no box due to 'list-style'»; `marker-computed-content.html`, вариант `none`).
2. `::marker` у элемента, не являющегося `list-item` (`li { display: block }`), — тоже пусто; Chrome отдаёт стиль псевдоэлемента (`marker-computed-content.html`, список `.no-list`).
3. `marker-computed-size.html` для `content: <image>` ждёт `100px`×`52px` (высота с выравниванием по базовой линии); у нас — высота строки.
4. Срез 25 BUG-1446 (`css/css-lists`: `marker-quotes`, `marker-webkit-text-fill-color`, `marker-dynamic-content-change`, `nested-marker-styling`, `marker-counter`) — маркер не получает свой стиль; пересекается с [BUG-1548](BUG-1548-OPEN.md) и BUG-1568.

## Что делать

Считать стиль `::marker` для запроса `getComputedStyle` независимо от бокса: для `list-item` без бокса отдавать каскадированный стиль маркера с `width`/`height: auto`, для не-`list-item` — стиль псевдоэлемента по правилам `::marker`. Проверить, что запись не раздувает `pseudo_computed_styles` на страницах без `::marker` (publish только по запросу, как `pseudo_styles_needed`).

## Как проверить

`css/css-pseudo/marker-computed-content.html`, `marker-computed-size.html`; `css/css-lists/marker-*`.
