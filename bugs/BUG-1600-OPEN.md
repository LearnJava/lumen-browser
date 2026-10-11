# BUG-1600 — CSS Logical Properties: порядок каскада логических и физических, `var()`, `CSS.supports`, валидация и `getComputedStyle`

**Статус:** OPEN
**Заведён:** 2026-10-11 (P2, WPT-RUN-14 срез 28, 30 малых модулей `css`: `css-logical`, `compositing`, `css-scroll-anchoring`, `css-content`, `css-scrollbars`, `css-will-change`, `css-syntax`, `css-color-adjust`, `css-rhythm`, `geometry` и др.)
**Область:** layout (`crates/engine/layout/src/style/logical.rs::resolve_logical_properties` — пост-каскадная подстановка; `selector_query.rs::computed_style_to_map`; `_lumen_css_supports_prop`)

## Симптом

Логические свойства разрешаются **после** каскада в физические, поэтому порядок записи и специфичность между `margin-left` и `margin-inline-start` теряются; часть свойств не видна `CSS.supports`/`getComputedStyle`; значения не проверяются.

## Проба (`--dump-layout`, сборка 2026-10-08)

| что | у нас | ожидается |
|---|---|---|
| `style="margin-left:1px;margin-inline-start:5px"` → `margin-left` | `1px` | `5px` (последнее объявление) |
| `style="width:1px;inline-size:3px"` → `width` | `1px` | `3px` |
| `style="inline-size:3px;width:1px"` → `width` | `1px` | `1px` |
| `.x{inline-size:3px}` + `style="width:1px"` | `1px` | `1px` (оба совпали; контроль) |
| `margin-inline-start: var(--one)` в `el.style` → `getPropertyValue` | `var(--one)` (ok); но `logicalprops-with-variables.html` падает в 9 из 12 подтестов (`Specified value expected "var(--one)" but got ""`) — путь теста не воспроизведён пробой | specified value `var(--one)` |
| `CSS.supports("border-block-start-width","2px")`, `("min-block-size","2px")`, `("overflow-block","hidden")`, `("border-inline-end-color","red")` | `false` (при том, что `style` принимает и `getComputedStyle` отдаёт `2px`/`rgb(255, 0, 0)`) | `true` |
| `style.blockSize = "none"` / `"-10px"` | принимается | отклоняется |
| `getComputedStyle(html).blockSize`, `border-block-end-color` (начальное) | `auto`, `currentcolor` | `0px`/`rgb(0, 0, 255)` по тесту `inheritance.html` (в нём 15 из 68 подтестов падают) |
| `getComputedStyle(el).getPropertyValue("margin-block-start")` при `writing-mode: vertical-rl` | `""` | значение |

## Охват

WPT-RUN-14 S28: `css/css-logical` — 92 id, зелёных 20. Не зелёные: `parsing/*` 37 id (243 из 360 подтестов), `logical-box-*`/`logicalprops-*`/`getComputedStyle-listing`/`inheritance` 19 id (462 из 596), `animation*`/`animations/*` 10 id (129 из 195; `float`/`clear` `inline-start|end` не интерполируются и не мапятся — 20 из 42), reftest `logical-values-float-clear-*` + `cascading-001` 6 id (`thick`). Прежде вместе — ≈72 id.

Каскад и `CSS.supports` — отдельные дефекты в одной записи: у обоих один владелец и один тестовый модуль; при починке разделить.

## Как проверить

`css/css-logical/logical-box-margin.html`, `logical-box-size.html`, `parsing/block-size-invalid.html`, `logicalprops-with-variables.html`, `cascading-001.html`.
