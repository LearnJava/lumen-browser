# BUG-1515 — `getComputedStyle()` не отдаёт `shape-outside`, `shape-margin`, `shape-image-threshold`, `text-decoration` (шорткод), `text-decoration-skip-ink`, `text-decoration-skip-spaces`, `text-decoration-inset`, `text-emphasis*`, `text-underline-position`; `text-decoration-color: currentcolor`, `text-underline-offset`, `text-decoration-thickness` и `text-shadow` отдаются неразрешёнными

**Статус:** OPEN
**Заведён:** 2026-10-08 (P2, WPT-RUN-14 срез 23, `css/css-text-decor` + `css-gaps` + `css-shapes`)
**Область:** layout (`crates/engine/layout/src/selector_query.rs::computed_style_to_map`)

## Симптом

Для перечисленных свойств `prop in getComputedStyle(el)` — `false`, `getPropertyValue()` — пустая строка, и тест падает на первом `assert_true(prop in cs)` или на `expected "…" but got ""`. Свойства, которые карта знает, отдаются неразрешёнными: `text-decoration-color: currentcolor` — `currentcolor` (ожидается `rgb(0, 255, 0)` при `color: lime`), `text-underline-offset: 1em` при `font-size: 40px` — `16px` (разрешено от 16 px, а не от размера элемента), `calc(10px - 8px)` и `200%` — `auto` / `2%` (дробь вместо процента), `text-decoration-line: blink` — `none`. Каждое отсутствующее свойство тянет за собой все сабтесты `*-computed`, `inheritance` и половину `values/shape-outside-*` (`expected "circle()" but got ""`). Смежно с BUG-1050 (`text-decoration-thickness`/`text-underline-offset` там записаны как «нет в карте» — теперь в карте, но значение неверное), BUG-1407, BUG-1435.

## Проба

Проба (`--mcp`, `<div id=t style="font-size:10px">`):

| вызов | у нас | ожидается |
|---|---|---|
| `"shape-outside" in gcs(t)` (так же `shape-margin`, `shape-image-threshold`, `text-decoration`, `text-emphasis`, `text-emphasis-style`, `text-emphasis-position`, `text-underline-position`) | `false` | `true` |
| `"text-decoration-skip-ink" in gcs(t)`, `CSS.supports("text-decoration-skip-ink","auto")` | `false`, `false` | `true`, `true` |
| `CSS.supports("text-decoration-skip-spaces","none")`, `("text-decoration-inset","1px")` | `false` | `true` |
| `style.textUnderlineOffset="1em"` при `font-size:40px`, `gcs(t).textUnderlineOffset` | `16px` | `40px` |
| `style.textUnderlineOffset="calc(10px - 8px)"`, `"200%"` | `auto`, `auto` | `2px`, `200%` |
| `style.textDecorationThickness="200%"`, `gcs` | `2%` | `200%` |
| `style.textDecorationLine="blink"`, `gcs` | `none` | `blink` |
| `color:rgb(0,255,0)` + `textDecorationColor="currentcolor"`, `gcs` | `currentcolor` | `rgb(0, 255, 0)` |
| `textShadow="10px 20px"` при `color:blue`, `gcs` | `currentcolor 10px 20px 0px` | `rgb(0, 0, 255) 10px 20px 0px` |

## Как найдено

WPT-RUN-14 срез 23: `css-shapes/parsing/*-computed`, `shape-functions/*-computed`, `shape-outside/values/*` (`computed`-варианты), `inheritance` (`css-shapes`, `css-text-decor`), `css-text-decor/parsing/*-computed`, `text-decoration-thickness-computed`, `text-underline-offset-computed`, `text-shadow/parsing/text-shadow-computed`. 83 id (кластеры «shape: getComputedStyle» 67 и «text-decor: getComputedStyle» 16), 2 905 из 4 501 сабтеста.

## Что делать

Добавить свойства в `computed_style_to_map` (значения — по CSS Values 4 §10 «resolved value»: абсолютные длины, `currentcolor` → цвет, `calc()` свёрнут); `text-underline-offset` и `text-decoration-thickness` резолвить от `font-size` самого элемента; записать `CSS.supports` для `text-decoration-skip-ink`. Парсер и каскад свойства знают (`apply/text.rs`, `apply/layout.rs`) — недостающее только в карте.

## Как проверить

Таблица выше; `css/css-shapes/parsing/shape-margin-computed.html`, `css/css-text-decor/parsing/text-underline-position-computed.html`, `css/css-text-decor/text-underline-offset-computed.html`.
