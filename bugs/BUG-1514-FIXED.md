# BUG-1514 — `getComputedStyle()` не отдаёт `container-type`, `container-name`, `container`; сериализация shorthand `container` не каноническая

**Статус:** FIXED 2026-10-24 (P3)
**Заведён:** 2026-10-08 (P2, WPT-RUN-14 срез 22, `css/css-view-transitions` + `css-conditional` + `css-variables` + `css-properties-values-api` + `css-mixins`)
**Область:** layout (`crates/engine/layout/src/selector_query.rs::computed_style_to_map` — `container-type`, `container-name`, `container`)

## Симптом

У элемента с `style="container-type:inline-size;container-name:foo bar"` `getComputedStyle(t).containerType`, `.containerName`, `.container` — пустые строки, `"container-type" in getComputedStyle(t)` и `"container-name" in …` — `false`, `getPropertyValue('container-type')` — пусто (`element.style.containerType` — `inline-size`, `style.cssText` верен; `style.container` пусто). Тесты на разбор (`container-parsing`, `container-type-parsing`, `container-name-parsing`) падают на канонической записи (`none / normal` вместо `none`, `inline- b / size`). Смежно с BUG-1401 (запись `container-type`).

## Проба

Проба (`--mcp`):

| вызов | у нас | ожидается |
|---|---|---|
| `gcs(t).containerType` | `` | `inline-size` |
| `gcs(t).containerName` | `` | `foo bar` |
| `gcs(t).container` | `` | `foo bar / inline-size` |
| `"container-type" in gcs(t)` | `false` | `true` |
| `t.style.container` | `` | `foo bar / inline-size` |
| `d.style.container = "none / normal"; d.style.container` | `none / normal` | `none` |

## Как найдено

WPT-RUN-14 срез 22: `container-queries/{container-computed,container-type-computed,container-name-computed,container-inheritance,container-ident-function,container-longhand-animation-type,container-name-parsing,container-type-parsing,container-parsing,at-container-parsing,container-name-tree-scoped}` — 11 id, 201 из 268 сабтестов; `scroll-state/container-type-scroll-state-computed` — ещё 1.

## Что делать

Добавить `container-type`/`container-name`/`container` в `computed_style_to_map` и сериализацию shorthand.

## Как проверить

Таблица выше; `css/css-conditional/container-queries/container-type-computed.html`, `container-computed.html`.

2026-10-21 (CQ-SCROLL-STATE): `computed_style_to_map` теперь отдаёт `container-type` (с `scroll-state`) и `container-name`; шорткат `container` и `'container-type' in getComputedStyle(t)` остаются.

2026-10-24 (P3): исправлено. `computed_style_to_map` отдаёт шорткат `container` (`name / type`, тип `normal` опускается, `none` без имени); в шиме `container` добавлен в `_LUMEN_TEXT_SHORTHANDS` (`_lumen_expand_container_shorthand`) — `element.style.container` разбирает и канонизирует значение, читает шорткат из longhand'ов, отвергает `none foo`, `foo and bar`, неверный тип. Тесты: `computed_map_container_shorthand` (layout), `container_shorthand_roundtrip` (js).
