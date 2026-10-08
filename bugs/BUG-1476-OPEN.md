# BUG-1476 — `display`: многословные значения (`inline flow-root`, `block ruby`, `list-item block`), `math`, `ruby`, `grid-lanes` и `initial`/`unset` на неблочных элементах дают `block`

**Статус:** OPEN
**Заведён:** 2026-10-08 (P2, WPT-RUN-14 срез 21, `css/css-anchor-position` + `css/css-position` + `css/css-display`)
**Область:** css-parser/layout (`crates/engine/layout/src/style/` — разбор и сериализация `display`; `selector_query.rs::computed_style_to_map`)

## Симптом

Значение, которого разбор не знает, превращается в `block` (`getComputedStyle(x).display === 'block'`) и `style.display` читается обратно как введённое. Не распознаются: многословные формы CSS Display 3 (`inline flow-root`, `block flow-root`, `list-item block`, `inline list-item`, `block ruby`, `inline grid`, `block flex` → `block`), `math`/`block math`/`inline math`, `ruby`/`ruby-text`, `grid-lanes`/`inline-grid-lanes`. Каноническая сериализация по спецификации — короткая форма (`block flow` → `block`, `inline flow-root` → `inline-block`, `block flow-root` → `flow-root`); у нас её нет, потому что нет разбора. `display: initial`/`unset` на `span`/`b`/`li`/`table` даёт `block` вместо `inline` (в `css-display/inheritance.html` ожидается `inline`; то же, что [BUG-1448](BUG-1448-OPEN.md) на `css-cascade/unset-val-002.html`). 11 id: 7 `css-display/parsing/*` и `animations/*` (из них `display-computed.html` — [BUG-1480](BUG-1480-OPEN.md), `display-invalid.html` — [BUG-1475](BUG-1475-OPEN.md)) и 4 `display-math-*`/`inheritance` — по имени и по сообщениям сабтестов: `display-valid.html` — 72 из 108 сабтестов («serialization should be canonical»: 22 `inline…`, 21 `ruby…`, 11 `flow…`), `display-interpolation*.html`, `tentative/display-valid.html` (3 из 6). `display-computed.html` (110 из 112) сюда не относится — это [BUG-1480](BUG-1480-OPEN.md).

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
