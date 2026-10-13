# BUG-1586 — `margin-trim` не реализовано: ни разбора, ни `getComputedStyle`, ни обрезки полей у блока, flex, grid и multicol

**Статус:** OPEN (ДОРАБОТКА → CSS-SPECS.md)
**Заведён:** 2026-10-09 (P2, WPT-RUN-14 срез 27, `css/css-ruby` + `css-layout-api` + `css-box` + `motion` + `css-highlight-api` + `css-paint-api` + `css-viewport` + `mediaqueries`)
**Область:** css-parser/layout (`grep margin-trim crates/` — ноль совпадений; `CSS.supports("margin-trim","block")` — `false`)

## Симптом

Свойства нет вовсе. `CSS.supports("margin-trim","block")` — `false`, `getPropertyValue("margin-trim")` — `""`, раскладка не меняется. Тесты `css-box/margin-trim/*` падают тремя группами: поля не обрезаются у блочного контейнера (36 id), у flex (23 id) и grid (7 id) и в multicol-разрыве (7 id); `computed-margin-values/*` (32 id) читают `getComputedStyle().marginTop` и ждут «использованное значение после обрезки» — `0` вместо `10`/`30`; три файла разбора и наследования (`inheritance.html`, `parsing/margin-trim.html`, `parsing/margin-trim-computed.html`).

## Проба

`--screenshot` 800×600, `body{margin:0}`, `#c{margin-trim:block;width:100px;background:blue}`, `#i{height:50px;margin:30px 0;background:green}`; `--dump-layout` + `console.log`:

| вызов | у нас | ожидается |
|---|---|---|
| верхний край зелёного бокса внутри `margin-trim:block` | `y=30` | `y=0` |
| `CSS.supports("margin-trim","block")`, `("margin-trim","inline-start")` | `false`, `false` | `true`, `true` |
| `el.style.marginTrim = "block"; el.style.getPropertyValue("margin-trim")` | `block` (запись в `style` принимает любую строку) | `block` |
| `getComputedStyle(el).getPropertyValue("margin-trim")` | `""` | `block` / `none` |
| `offsetTop` первого ребёнка flex-колонки с `margin-trim: block`, `margin: 10px` | `18` | `8` |

## Как найдено

WPT-RUN-14 срез 27: `css/css-box/margin-trim/*` (105 id), `css-box/inheritance.html`, `css-box/parsing/margin-trim*.html`.

## Что делать

Новая строка в `CSS-SPECS.md` (P4 разбирает свойство сквозным путём, как остальные): разбор `none | [ block | inline ] | [ block-start || block-end || inline-start || inline-end ]`, не наследуется, `ComputedStyle`, `getComputedStyle`; обрезка — использованное значение поля, а не вычисленное (CSS Box 4 §«Trimming margins»): у блочного контейнера — поля первого/последнего ребёнка по оси (с учётом схлопывания с самосхлопывающимися потомками и border/padding контейнера), у flex/grid — поля элементов у края контейнера (в grid — не у схлопнутых дорожек), в multicol — у разрывов колонок. Порядок: сначала блочный контейнер (36 id), затем flex (23 id), grid (7), multicol (7).

## Как проверить

`css/css-box/margin-trim/block-container-block-start-001.html`, `margin-trim/flex-block-trimmed-only.html`, `computed-margin-values/block-container-block-end.html`.
