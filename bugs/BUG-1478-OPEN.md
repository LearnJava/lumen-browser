# BUG-1478 — `getComputedStyle()` не отдаёт свойства CSS Anchor Positioning: `anchor-name`, `position-anchor`, `position-area`, `anchor-scope` и значения `anchor()`/`anchor-size()`

**Статус:** OPEN
**Заведён:** 2026-10-08 (P2, WPT-RUN-14 срез 21, `css/css-anchor-position` + `css/css-position` + `css/css-display`)
**Область:** layout/js (`crates/engine/layout/src/selector_query.rs::computed_style_to_map` — нет строк для `anchor-name`, `position-anchor`, `position-area`, `anchor-scope`; `getComputedStyle` anchor-свойств)

## Симптом

Свойства разбираются и работают в раскладке, а `getComputedStyle(el).getPropertyValue(...)` для них — пустая строка (`'anchor-name' in getComputedStyle(el)` — `false`): `anchor-name`, `position-anchor`, `position-area`, `anchor-scope`, `position-try*`, `position-visibility`. Для `top`/`left` с `anchor()` отдаётся используемое значение вместо вычисленного (`getComputedStyle(x).left` → `140px` при `left:anchor(--a right)`; WPT `anchor-getComputedStyle-*` ждёт использованное значение и для `bottom`/`right` без `auto`-резолва, но проверяет и пустые). 26 id (все `OK` с упавшими сабтестами, 5 641 из 10 358): `anchor-parse-valid.html` 1 206 из 2 359, `anchor-size-parse-valid.html` 1 493 из 4 305, `position-area-parsing.html` 1 800 из 2 125 и `position-area-computed.html` 633, `property-interpolations.html` 294 из 546, `anchor-scope-computed.html`, `position-visibility-computed.html`, `anchor-getComputedStyle-001…004`, `anchor-ident-function.html`. Большая часть сабтестов `parse-valid` упирается в `style.setProperty` для `anchor()` внутри `calc()` (BUG-563, GAP-ANCHORCSSOM).

## Проба

Проба (`--mcp`, `style.cssText='anchor-name:--x;position-anchor:--x;position-area:top left;anchor-scope:all'`):

| проверка | у нас | ожидается |
|---|---|---|
| `style.getPropertyValue('anchor-name')` | `--x` | `--x` |
| `getComputedStyle(e).getPropertyValue('anchor-name')` | `` | `--x` |
| то же для `position-anchor`, `position-area`, `anchor-scope` | `` | `--x`, `top left`, `all` |
| `'anchor-name' in getComputedStyle(e)` | `false` | `true` |
| `CSS.supports('left','anchor(--a left)')` | `true` | `true` |
| `e.style.left='anchor(--a left, anchor(right))'; e.style.left` | `` (отклонено) | принято, `anchor(--a left, anchor(right))` |
| `e.style.left='calc(anchor(--a left) + 5px)'` | `` (отклонено) | принято |

## Как найдено

WPT-RUN-14 срез 21: `css-anchor-position/anchor-getComputedStyle-002.html`, `position-area-computed.html`, `anchor-scope-computed.html`.

## Что делать

Добавить свойства в карту `computed_style_to_map` (тот же механизм, что [BUG-472](BUG-472-OPEN.md), [BUG-1297](BUG-1297-OPEN.md), [BUG-1402](BUG-1402-OPEN.md)); `anchor()` внутри `calc()` и вложенный фолбэк — GAP-ANCHORCSSOM ([BUG-563](BUG-563-OPEN.md)).

## Как проверить

Таблица выше; `css/css-anchor-position/anchor-getComputedStyle-002.html`, `position-area-computed.html`.
