# BUG-1476 — `display`: многословные значения (`inline flow-root`, `block ruby`, `list-item block`), `math`, `ruby`, `grid-lanes` и `initial`/`unset` на неблочных элементах дают `block`

**Статус:** FIXED 2026-10-22 (P3)
**Заведён:** 2026-10-08 (P2, WPT-RUN-14 срез 21, `css/css-anchor-position` + `css/css-position` + `css/css-display`)
**Область:** css-parser/layout (`crates/engine/layout/src/style/` — разбор и сериализация `display`; `selector_query.rs::computed_style_to_map`)

## Симптом

Значение, которого разбор не знает, превращается в `block` (`getComputedStyle(x).display === 'block'`) и `style.display` читается обратно как введённое. Не распознаются: многословные формы CSS Display 3 (`inline flow-root`, `block flow-root`, `list-item block`, `inline list-item`, `block ruby`, `inline grid`, `block flex` → `block`), `math`/`block math`/`inline math`, `ruby`/`ruby-text`, `grid-lanes`/`inline-grid-lanes`. Каноническая сериализация по спецификации — короткая форма (`block flow` → `block`, `inline flow-root` → `inline-block`, `block flow-root` → `flow-root`); у нас её нет, потому что нет разбора. `display: initial`/`unset` на `span`/`b`/`li`/`table` даёт `block` вместо `inline` (в `css-display/inheritance.html` ожидается `inline`; то же, что [BUG-1448](BUG-1448-FIXED.md) на `css-cascade/unset-val-002.html`). 11 id: 7 `css-display/parsing/*` и `animations/*` (из них `display-computed.html` — [BUG-1480](BUG-1480-FIXED.md), `display-invalid.html` — [BUG-1475](BUG-1475-OPEN.md)) и 4 `display-math-*`/`inheritance` — по имени и по сообщениям сабтестов: `display-valid.html` — 72 из 108 сабтестов («serialization should be canonical»: 22 `inline…`, 21 `ruby…`, 11 `flow…`), `display-interpolation*.html`, `tentative/display-valid.html` (3 из 6). `display-computed.html` (110 из 112) сюда не относится — это [BUG-1480](BUG-1480-FIXED.md).

## Проба

Проба (`--mcp`, `getComputedStyle(div).display` после `style.display = v`):

| `v` | у нас | ожидается |
|---|---|---|
| `inline flow-root` | `block` | `inline-block` |
| `block flow-root` | `block` | `flow-root` |
| `list-item block` | `block` | `list-item` |
| `inline list-item` | `block` | `inline list-item` |
| `ruby`, `ruby-text`, `math`, `grid-lanes` | `block` | то же слово |
| `inline grid`, `block flex` | `block` | `inline-grid`, `flex` |
| `contents` | `` (пустая строка) | `contents` |
| `flow-root`, `table-row`, `table-caption` | верно | — |

## Как найдено

WPT-RUN-14 срез 21: `css-display/parsing/display-valid.html`, `display-computed.html`, `inheritance.html`, `display-math-on-non-mathml-elements.html`.

## Что делать

Разобрать `<display-outside> || <display-inside>`, `<display-listitem>`, `<display-legacy>` и `ruby`/`math`/`grid-lanes` в `Display`; сериализовать каноническую короткую форму; отдавать `contents` в computed. Пересекается с BUG-1448 (`initial`/`unset`) — чинить вместе.

## Как проверить

Таблица выше; `css/css-display/parsing/display-valid.html`.

## Исправление

- `crates/engine/layout/src/style/parse/display.rs::parse_display` — грамматика `display` CSS Display 3 §2 (внешнее/внутреннее слово в любом порядке, `list-item` с `flow`/`flow-root`, `run-in`, `ruby`/`math`, `ruby-base|text|*-container`, `grid-lanes`); недопустимое значение отбрасывается. Формы, которых нет в `Display` (`inline list-item`, `ruby`, `block math`, `run-in flex`, `ruby-text`), сохраняются в `ComputedStyle::display_form`, `webkit_box_computed_display` отдаёт их в `getComputedStyle`.
- `crates/js/src/shim/web_api_shim_mid.js::_lumen_css_canonical_display` — `style.display` читается в короткой канонической форме (`inline flow-root` → `inline-block`) и отбрасывает невалидное значение.
- `initial`/`unset` на неблочных элементах — уже исправлено в [BUG-1448](BUG-1448-FIXED.md).
- Остаток: `contents`/`inline` у элемента без бокса в `getComputedStyle` сразу после присваивания читается пустым — это [BUG-1480](BUG-1480-FIXED.md) (карта computed не содержит элемент без бокса). Блокификация `ruby`/`inline list-item` в flex/grid-элементе даёт короткую форму (`block`/`list-item`), а не `block ruby`.
