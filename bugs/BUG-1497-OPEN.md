# BUG-1497 — Условия `@container`: диапазонный синтаксис, `not`, запятая, `orientation`/`aspect-ratio`, логические оси, `calc()`/`var()`/`vw`/`rem`, `(width)` и регистр имён — все ложны

**Статус:** OPEN
**Заведён:** 2026-10-08 (P2, WPT-RUN-14 срез 22, `css/css-view-transitions` + `css-conditional` + `css-variables` + `css-properties-values-api` + `css-mixins`)
**Область:** layout/css-parser (`crates/engine/layout/src/style/container.rs:185` `evaluate_container_condition` — разбор условия размера)

## Симптом

`evaluate_container_condition` понимает `(min-width|max-width|width|min-height|max-height|height: <px|em>)` и `and`/`or`; остальное «неизвестная фича → false». Не работают: `not (min-width:300px)`; список через запятую; диапазонный синтаксис `(width >= 100px)`, `(100px < width < 300px)`, `(200px >= width)`; `orientation`, `aspect-ratio`, `min-aspect-ratio`; логические `inline-size`/`block-size`/`min-inline-size`/`min-block-size`; значения с `calc()`, `var()`, `vw`, `rem`; булева форма `(width)`/`(height)`; имя признака в верхнем регистре `(MIN-WIDTH:100px)`; вложенное `not ((a) or (b))`.

## Проба

Проба (`--mcp`, `.c{container-type:size;width:200px;height:100px}`, «применяется» = `green`):

| запрос | у нас | ожидается |
|---|---|---|
| `(min-width:100px)`, `(min-width:100px) and (min-height:50px)`, `(min-width:300px) or (min-height:50px)`, `(min-width:10em)`, `( min-width : 100px )` | применяется | применяется |
| `not (min-width:300px)` | **нет** | применяется |
| `(min-width:300px), (min-height:50px)` | **нет** | применяется |
| `(width >= 100px)`, `(width > 100px)`, `(width <= 200px)`, `(width = 200px)`, `(200px >= width)`, `(100px < width < 300px)` | **нет** | применяется |
| `(orientation:landscape)`, `(aspect-ratio:2/1)`, `(min-aspect-ratio:1/1)`, `(aspect-ratio >= 1)` | **нет** | применяется |
| `(inline-size >= 100px)`, `(block-size >= 50px)`, `(min-inline-size:100px)`, `(min-block-size:50px)` | **нет** | применяется |
| `(min-width:calc(100px + 50px))`, `(min-width:50vw)`, `(min-width:10rem)`, `(min-width:var(--w))` | **нет** | применяется |
| `(width)`, `(height)`, `(MIN-WIDTH:100px)` | **нет** | применяется |
| `not ((min-width:300px) or (min-height:500px))` | **нет** | применяется |
| `(orientation:portrait)`, `foo(bar)` | нет | нет |

## Как найдено

WPT-RUN-14 срез 22: `container-queries/{size-feature-evaluation,query-evaluation,aspect-ratio-feature-evaluation,calc-evaluation,viewport-units,var-evaluation,font-relative-*,comma-separated-*,multiple-conditions-001,size-query-with-var,query-content-box,percentage-padding-orthogonal}` — 51 id (36 OK с падениями, 15 reftest `thick`); сабтесты `size-feature-evaluation` 28 из 56, `query-evaluation` 13 из 38.

## Что делать

Разбор условия по Conditional 5 §size-container-features: `<mf-range>`, `not`/`and`/`or`, список через запятую (OR), логические оси по `writing-mode` контейнера, `calc()`/`var()`/относительные единицы, регистронезависимые имена.

## Как проверить

Таблица выше; `css/css-conditional/container-queries/size-feature-evaluation.html`, `query-evaluation.html`.
