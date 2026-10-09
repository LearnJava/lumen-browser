# BUG-1578 — CSS Shadow Parts не реализован: `::part()`, атрибут `part`, `Element.part`, `exportparts`

**Статус:** OPEN (ДОРАБОТКА → SHADOW-PARTS)
**Заведён:** 2026-10-09 (P2, WPT-RUN-14 срез 26, `css/css-page` + `css-animations` + `css-transitions` + `css-shadow` + `css-borders` + `css-scroll-snap`)
**Область:** css-parser/layout/js (ноль совпадений по `::part` в `crates/engine/`; `Element.part` отсутствует; `CSS.supports("selector(::part(a))")` — `false`)

## Симптом

`::part()`, `part`, `exportparts` и `Element.part` отсутствуют целиком. Тесты `css-shadow/part/*` падают дважды: сначала на BUG-1577 (хелпер не находит узел), а после его обхода — на `rgb(0, 0, 0)` вместо `rgb(0, 128, 0)`.

## Проба

`--dump-layout` + `console.log`:

| вызов | у нас | ожидается |
|---|---|---|
| `#h1::part(p1){color:rgb(255,0,0)}`, `<span part=p1>` в shadow tree `#h1` | `rgb(0, 0, 0)` | `rgb(255, 0, 0)` |
| `x-h::part(p2){color:blue}`, `part="p1 p2"` | `rgb(0, 0, 0)` | `rgb(0, 0, 255)` |
| `CSS.supports("selector(::part(a))")`, `("selector(x::part(a b))")` | `false`, `false` | `true`, `true` |
| `typeof span.part`, `span.part.length` | `undefined` | `object`, 1…n |
| `exportparts="inner:outer"` у вложенного хоста | не читается | форвардинг имён |

## Как найдено

WPT-RUN-14 срез 26: `css/css-shadow/part/*` (48 id: 2 зелёных).

## Что делать

Новая функциональность (задача SHADOW-PARTS): разбор `::part(<ident>+)` с хвостом из псевдоклассов и псевдоэлементов, сопоставление через `part` и `exportparts` (включая вложенные хосты), каскад с учётом области (BUG-1580), `Element.part`, `CSS.supports`/`insertRule` для селектора, инвалидация при смене `part`.

## Как проверить

`css/css-shadow/part/simple.html`, `simple-forward.html`, `exportparts-layered.html`, `part-name-idl.html`.
