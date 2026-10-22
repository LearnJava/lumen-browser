# BUG-1446 — `::marker`: `getComputedStyle(li, '::marker')` отдаёт значения элемента, а не маркера (`content: normal`, `display: list-item`, `unicode-bidi: normal`, ширина `1008px`), а `letter-spacing` и `text-transform` на маркере не применяются

**Статус:** FIXED 2026-10-21 (P3)
**Заведён:** 2026-10-08 (P2, WPT-RUN-14 срез 20, `css/selectors` + `css-pseudo` + `css-nesting` + `css-namespaces` + `css-cascade`)
**Область:** layout/paint (`::marker`: стиль и компьютед; `crates/engine/layout/src/style/`, `crates/engine/layout/src/box_tree/`)

## Симптом

(1) Computed `::marker` — не стиль маркера: при `li::marker{content:"string"}` `content` = `normal`, `display` = `list-item` (ожидается `inline`), `unicode-bidi` = `normal` (ожидается `isolate`), `font-variant-numeric` = `normal` (`tabular-nums`), `text-transform` = `lowercase` против `none`, `text-indent` = `1px` против `0px`, `width` = `1008px` (у `::before` с тем же `content` computed верен). `marker-default-styles.html` — 32 из 32 сабтестов падают, `marker-computed-content` 6 из 10, `marker-computed-size` 8 из 8, `marker-display-computed` 8 из 8. (2) Отрисовка текста маркера: `content:"abc"` даёт `DrawText … "abc"` шириной 32; `letter-spacing:10px` — 32 (без изменений), `text-transform:uppercase` — те же 32 и нижний регистр; `word-spacing` учитывается. `text-shadow` на маркере не проверялся (формат тени в `--dump-display-list` не установлен). Сам `content` и `color` рисуются. 43 id среза (`marker-*`, `marker-computed-*`: 33 reftest `thick`, 9 testharness).

## Проба

| проверка | у нас | ожидается |
|---|---|---|
| `getComputedStyle(li,"::marker").content` при `li::marker{content:"string"}` | `normal` | `"string"` |
| `.display` | `list-item` | `inline` |
| `.unicodeBidi` | `normal` | `isolate` |
| `.color` при `li::marker{color:green}` | `rgb(0, 0, 0)` | `rgb(0, 128, 0)` |
| `.fontSize` при `li::marker{font-size:20px}` | `16px` | `20px` |
| `--dump-display-list`, `content:"abc"` | `DrawText "abc"` 32 px | верно |
| то же, `letter-spacing:10px` | 32 px | 62 px |
| то же, `text-transform:uppercase` | `"abc"` | `"ABC"` |
| `--screenshot`, `li::marker{color:green}` без `content` | 192 зелёных px | верно |

`::before` с тем же `content` отдаёт `"string"`, `inline`, `rgb(0, 128, 0)`.

## Как найдено

WPT-RUN-14 срез 20: `css-pseudo/marker-default-styles.html` (32/32), `marker-computed-size.html`, `marker-letter-spacing.html`, `marker-text-transform-uppercase.html`.

## Что делать

(1) Читать computed `::marker` из стиля псевдоэлемента (как для `::before`), а не из `li`; UA-таблица маркера по Pseudo 4 §4.2 (`unicode-bidi: isolate`, `font-variant-numeric: tabular-nums`, `text-transform: none`, `text-indent: 0`). (2) Прогонять текст маркера через тот же путь, что и обычный текстовый ран: letter-spacing, text-transform (и проверить text-shadow).

## Как проверить

Таблица выше; `css/css-pseudo/marker-default-styles.html`, `marker-computed-content.html`.

## Срез 25 (2026-10-08, P2, WPT-RUN-14 `css/css-typed-om` + `cssom-view` + `cssom` + `css-lists` + `css-counter-styles`)

`css/css-lists`: `marker-quotes`, `marker-webkit-text-fill-color`, `marker-dynamic-content-change`, `nested-marker-styling`, `marker-counter` — маркер не получает свой стиль. Пересекается с BUG-1548 (ширина `inside`-маркера) и BUG-1568.

## Исправление (2026-10-21, P3)

- `crates/engine/layout/src/style/pseudo.rs` — новая `marker_base_style`: база псевдоэлемента плюс UA-значения Pseudo L4 §4.2 (`unicode-bidi: isolate`, `font-variant-numeric: tabular-nums`, `text-transform: none`, `text-indent: 0`) — наследуемые `text-transform`/`text-indent` маркер больше не берёт у `li`. `marker_property_applies` пропускает `text-transform`, `letter-spacing`, `word-spacing`, `text-shadow`, `text-emphasis*`, `tab-size`, `hyphens`, `line-break`, `word-break`, `overflow-wrap` (до этого правило `::marker { letter-spacing }` отбрасывалось целиком).
- `crates/engine/layout/src/box_tree/inline_build.rs::inject_marker` — строка маркера проходит `text_transform` (регистр, `full-width` и т. д.).
- `crates/engine/paint/src/display_list/form_controls.rs::emit_list_marker` — `DrawText` маркера получает `letter_spacing`/`word_spacing` (раскладка уже мерила ширину с `letter-spacing`, рисовалось без).
- `crates/engine/layout/src/lib.rs::collect_pseudo_computed_styles_box` — запись `"marker"` для каждого `BoxKind::Marker` (`marker_style_map`: `display` = `inline` у `inside`, `block` у `outside`; `width`/`height` — размер бокса маркера); шим `_lumen_normalize_pseudo_elt` принимает `::marker`. Раньше имя не распознавалось, и `getComputedStyle(li, '::marker')` отдавал стиль самого `li`.

## Проверка

- `crates/driver/tests/cases/bug1446_marker_computed_style.rs` — 3 теста (UA-значения при `li { text-transform: lowercase; text-indent: 1px }`, правило `::marker` читается, `display`/`width` у маркера); до правки падали.
- `crates/engine/layout/src/tests/half_leading_columns_floats.rs` — UA-значения, `text-transform` меняет строку маркера, `letter-spacing` доходит до стиля маркера. Тест `marker_ignores_non_applicable_properties` переведён с `letter-spacing` на `text-decoration-line`: WPT `marker-letter-spacing.html` требует, чтобы `letter-spacing` на `::marker` работал.
- Перезапуск WPT-категории `css-pseudo`/`css-lists` не выполнялся: `.ini` для `marker-default-styles`, `marker-computed-*`, `marker-letter-spacing`, `marker-text-transform-*` снимать отдельным прогоном.

## Остаток → [BUG-1667](BUG-1667-OPEN.md)

Маркер без бокса (`list-style: none`, `content: none`) и `::marker` у не-`list-item` отдают пустую строку вместо `auto`/своего стиля; `marker-computed-content`, `marker-computed-size` (размер для `content: <image>`), срез 25 `css-lists`.
