# BUG-1377 — невалидное объявление `width`/`height` заменяет предыдущее вместо того, чтобы быть отброшенным (CSS Syntax §2.2, CSS 2.1 §4.2)

**Статус:** OPEN
**Заведён:** 2026-10-07 (P2, WPT-RUN-14 срез 15, `css/CSS2` (остальные каталоги: selectors, css1, syntax, box-display, visufx, visudet, …))
**Область:** css-parser/layout (`crates/engine/layout/src/style/apply/layout.rs:293` — `width`/`height` с невалидным значением; `color` с `rgb(100%, 0, 0)`)

## Симптом

CSS 2.1 §4.1.8 / CSS Syntax: объявление с невалидным значением игнорируется целиком, действует предыдущее. В `apply_declaration` ветки `"width"`/`"height"` пишут `style.width = parse_sizing_length(val, is_quirks)` — результат `None` (невалидно) неотличим от `auto`, и значение сбрасывается.

`--dump-layout`, `div{background:green;width:20px;height:20px;<decl>}`:

| `<decl>` | высота `div` | ожидается |
|---|---|---|
| `height:foo` | 0 | 20 |
| `height:1 0px` | 0 | 20 |
| `height:+ 10px` | 0 | 20 |
| `height:-5px` | 0 | 20 |
| `width:foo` (после `width:20px`) | 1024 (`auto`) | 20 |
| `margin-top:foo` после `margin-top:20px` | 20 | 20 — **верно** |
| `padding-top:foo` после `padding-top:20px` | 20 | **верно** |
| `max-width:foo` после `max-width:30px` | 30 | **верно** |
| `height:+20px` | 20 | **верно** |

То есть дефект — в `width`/`height` (и, судя по `inline-size`/`block-size` на `layout.rs:314,317`, в логических), у `min-*`/`max-*`/`margin`/`padding` проверка есть.

Отдельно: `syntax/colors-004.xht` (`color:rgb(100%, 0, 0)` — смешение процентов и чисел, в CSS 2.1 невалидно) — не пробовалось, причина не отделена.

## Как найдено

WPT-RUN-14 срез 15: `css/CSS2/syntax/signed-numbers-001.xht`, `css/CSS2/values/numbers-units-006.xht` (`height:1in; height:-1px`), `css/CSS2/box-display/root-box-002.xht`; `syntax/colors-004.xht`, `syntax/keywords-000.xht` (`background:"red"` — строка вместо цвета; у `color` строка отбрасывается верно).

## Что делать

`width`/`height` (и `inline-size`/`block-size`): если `parse_sizing_length` вернул `None` и значение не `auto` — оставить предыдущее. Единообразие — по образцу `min-width` на `layout.rs:333`.

## Как проверить

`css/CSS2/syntax/signed-numbers-001.xht`, `css/CSS2/values/numbers-units-006.xht`.
