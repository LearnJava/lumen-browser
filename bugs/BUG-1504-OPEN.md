# BUG-1504 — Анимация зарегистрированного свойства: списки (`<length>+`, `<length>#`), `<color>`, `<transform-function>` и `<transform-list>` не интерполируются — берётся конечное значение

**Статус:** OPEN
**Заведён:** 2026-10-08 (P2, WPT-RUN-14 срез 22, `css/css-view-transitions` + `css-conditional` + `css-variables` + `css-properties-values-api` + `css-mixins`)
**Область:** layout (`crates/engine/layout/src/animation.rs` — интерполяция значений зарегистрированных пользовательских свойств)

## Симптом

`CSS.registerProperty` + `element.animate({'--p':[a,b]})` интерполирует `<length>`, `<number>`, `<percentage>`, `<angle>` (в том числе с `composite:'add'`). Для `<length>+`/`<length>#`, `<color>`, `<transform-function>` результат на середине анимации равен конечному значению (как у дискретной), а `iterationComposite:'accumulate'` у списков не накапливает. `<custom-ident>` и `<url>` дискретны — верно, но падают на начальном значении (BUG-1505).

## Проба

Проба (`--mcp`, `animate`, `currentTime = 500` из 1000 мс, `getComputedStyle(target).getPropertyValue(name)`):

| синтаксис, ключевые кадры | у нас | ожидается |
|---|---|---|
| `<length>`: `100px` → `200px` | `150px` | `150px` |
| `<number>`, `<percentage>`, `<angle>` | `150`, `150%`, `150deg` | то же |
| `<length>` с `composite:add`, `200px`→`300px` на `100px` | `250px` | `250px` |
| `<length>+`: `100px 200px` → `200px 300px` | `200px 300px` | `150px 250px` |
| `<length>#`: `100px, 200px` → `200px, 300px` | `200px, 300px` | `150px, 250px` |
| `<color>`: `rgb(100,100,100)` → `rgb(200,200,200)` | `rgb(200,200,200)` | `rgb(150, 150, 150)` |
| `<transform-function>`: `translateX(100px)` → `translateX(200px)` | `translateX(200px)` | `translateX(150px)` |
| одиночный кадр `'200px'` (строка) при `initial-value:100px` | `` | `150px` |

## Как найдено

WPT-RUN-14 срез 22: `css-properties-values-api/animation/custom-property-animation-*` и `registered-*` — 39 id; сабтесты: списки (`*-comma-list`, `*-space-list`, 18 файлов × 6), `<color>`, `<transform-*>`, `*-with-iterationComposite` (30), `*-single-keyframe` (30, из них — начальное значение, BUG-1505).

## Что делать

Интерполяция по типу синтаксиса из регистрации: списки — поэлементно (при совпадении длин), `<color>`, `<transform-function>`/`<transform-list>` — как у стандартных свойств; `iterationComposite`.

## Как проверить

Таблица выше; `css/css-properties-values-api/animation/custom-property-animation-length-space-list.html`, `custom-property-animation-color.html`.
