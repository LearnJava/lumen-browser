# BUG-1582 — `animation-*` и `transition-*`: разбор и сериализация longhand-ов — невалидное принимается, шорткод не канонизируется

**Статус:** OPEN
**Заведён:** 2026-10-09 (P2, WPT-RUN-14 срез 26, `css/css-page` + `css-animations` + `css-transitions` + `css-shadow` + `css-borders` + `css-scroll-snap`)
**Область:** css-parser/layout/js (`crates/engine/layout/src/selector_query.rs::computed_style_to_map`; разбор `animation`/`transition` в `style/parse/`)

## Симптом

CSSOM-слой longhand-ов анимаций и переходов — тот же класс дефектов, что BUG-1278 (computed), BUG-1325 (CSS Text) и BUG-1525 (gap): значения принимаются без проверки области, шорткод не раскладывается.

## Проба

`--dump-layout` + `console.log`, `element.style` / `getComputedStyle`:

| вызов | у нас | ожидается |
|---|---|---|
| `style.animationDelay = "infinite"` | `infinite` | `""` |
| `style.transitionDuration = "-1s"` | `-1s` | `""` |
| `style.transitionTimingFunction = "steps(2,start)"` | `steps(2,start)` | `steps(2, start)` |
| `style.transitionTimingFunction = "step-start"` | `step-start` | `steps(1, start)` (в computed) |
| `style.transitionProperty = "ALL"` | `ALL` | `all` |
| `getComputedStyle(a).animation`, `.transition` при заданных longhand-ах | `""` | `k 2s ease-in 1s 3 alternate both paused` и т. п. |
| `getComputedStyle(el).transitionTimingFunction` при `steps(N)` | `steps(N, jump-end)` | `steps(N)` (сообщение теста `transition-timing-function-computed`) |
| `getComputedStyle(a).outlineOffset`, `.outline` | `""` | `3px`, `2px solid …` (BUG-1278) |

## Как найдено

WPT-RUN-14 срез 26: `css/css-animations/parsing/*` (31 id), `css/css-transitions/parsing/*` (15 id), `inheritance.html` в обоих модулях.

## Что делать

Проверять область значений при записи, канонизировать `steps()`/ключевые слова `step-start|step-end`/регистр `all`/`none`, добавить шорткоды `animation` и `transition` в `computed_style_to_map` (полный разбор по спискам с выравниванием длин).

## Как проверить

`css/css-animations/parsing/animation-delay-invalid.html`, `css/css-transitions/parsing/transition-timing-function-valid.html`.
