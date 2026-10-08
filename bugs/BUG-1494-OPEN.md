# BUG-1494 — `@supports selector()`, `at-rule()` и `font-tech()`: `selector()` не знает ряда псевдоэлементов, `selector(a, b)` принимается, `at-rule()` всегда ложно

**Статус:** OPEN
**Заведён:** 2026-10-08 (P2, WPT-RUN-14 срез 22, `css/css-view-transitions` + `css-conditional` + `css-variables` + `css-properties-values-api` + `css-mixins`)
**Область:** css-parser/layout (`crates/engine/css-parser/src/parser/supports.rs` — `selector()`, `at-rule()`, `font-tech()`)

## Симптом

`@supports selector(input::file-selector-button)`, `selector(input::details-content)`, `selector(input::details-content::before)`, `selector(input::-webkit-slider-thumb)` дают «не поддерживается», хотя движок эти псевдоэлементы знает (или обязан знать по тесту); `selector(div, div)` (список селекторов — недопустимо) даёт «поддерживается»; `at-rule(@media)`, `at-rule(@supports)` ложны — функции `at-rule()` нет. `font-tech(color-COLRv1)` ложно (список технологий шрифта короче нужного); `font-format(opentype|truetype|woff)` верны.

## Проба

Проба (`--mcp`, `@supports ⟨f⟩{#t{color:green}}`, фон `red`):

| условие | у нас | ожидается |
|---|---|---|
| `selector(a > b)` | применяется | применяется |
| `selector(input::file-selector-button)` | **нет** | применяется |
| `selector(input::details-content)`, `selector(input::details-content::before)` | **нет** | применяется |
| `selector(input::-webkit-slider-thumb)` | **нет** | применяется |
| `selector(input::-webkit-asdf)`, `selector(::bogus)`, `selector(a:bogus)` | нет | нет |
| `selector(div, div)` | применяется | нет |
| `at-rule(@media)`, `at-rule(@supports)` | **нет** | применяется |
| `at-rule(@bogus)` | нет | нет |
| `font-tech(color-COLRv1)` | **нет** | применяется |
| `font-tech(features-opentype)`, `font-format(opentype)` | применяется | применяется |

## Как найдено

WPT-RUN-14 срез 22: 7 reftest `css-conditional/at-supports-{selector-004,selector-details-content,selector-details-content-before,selector-file-selector-button,selector-webkit-slider-thumb.tentative,font-format-001,font-tech-001}` (последние два — `font-tech(color-COLRv1)` и смежное) и `js/supports-at-rule.html` (15 из 25 сабтестов).

## Что делать

Список известных псевдоэлементов для `selector()` брать из матчера селекторов, а не из отдельной таблицы; список в `selector()` — недопустим; добавить `at-rule(<name>)` по списку реализованных at-правил; дополнить список `font-tech`.

## Как проверить

Таблица выше; `css/css-conditional/at-supports-selector-004.html`, `js/supports-at-rule.html`.
