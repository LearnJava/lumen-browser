# BUG-1580 — Каскад «документ против shadow tree» не соблюдает порядок областей: правила `:host` побеждают правила документа

**Статус:** OPEN
**Заведён:** 2026-10-09 (P2, WPT-RUN-14 срез 26, `css/css-page` + `css-animations` + `css-transitions` + `css-shadow` + `css-borders` + `css-scroll-snap`)
**Область:** layout (`crates/engine/layout/src/` — каскад по областям encapsulation context; CSS Scoping 1 / CSS Cascade 5 §6.4)

## Симптом

Нормальное правило `:host` из shadow tree участвует в общем каскаде как правило документа и выигрывает по порядку/специфичности. По спецификации сначала сравнивается контекст инкапсуляции: для нормальных деклараций побеждает внешний контекст (документ), для `!important` — внутренний.

## Проба

`--dump-layout` + `console.log`, `my-host{display:block;…}` в документе, `:host{…}` в shadow `<style>`; замер `getComputedStyle(h).background-color`:

| документ | shadow `:host` | у нас | ожидается |
|---|---|---|---|
| `background:red` | `background:green` | green | red |
| `background:green` | `background:red` | red | green |
| `background:red` | `background:green !important` | green | green (контроль: верно) |
| `background:red !important` | `background:green !important` | green | green (контроль: у `!important` побеждает внутренний контекст) |
| `color:red` (селектор `my-host`, специфичность ниже) | `color:green` | green | red |

## Как найдено

WPT-RUN-14 срез 26: `css/css-shadow/css-scoping-shadow-host-rule.html` (reftest `thick`), `shadow-cascade-order-001.html` (`AN. document vs ::slotted, document rule should win`, 16 из 64).

## Что делать

Для нормальных деклараций сравнивать контекст инкапсуляции раньше специфичности и порядка (`Cascade 5 §6.4`); применить к `:host`, `::slotted()`, `::part()` (после SHADOW-PARTS).

## Как проверить

`css/css-shadow/css-scoping-shadow-host-rule.html`, `shadow-cascade-order-001.html`.
