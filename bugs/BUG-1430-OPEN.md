# BUG-1430 — Математические функции (`calc()`, `min()`, `abs()`, `round()`, `sin()`, `progress()`…) принимаются только для `<length>`; для `<number>`/`<integer>`/`<time>`/`<angle>`/`<percentage>`, константы `pi`/`e`/`infinity`/`NaN` — нет; сериализация не сводится к `calc()`

**Статус:** OPEN
**Заведён:** 2026-10-08 (P2, WPT-RUN-14 срез 19, `css/css-images + css/css-values + css/css-color`)
**Область:** layout/css-parser (`crates/engine/layout/src/style/parse/` — `calc()`/`min()`/`round()`… в нечисловых контекстах; `selector_query.rs`)

## Симптом

`getComputedStyle(el).getPropertyValue(prop)` после `el.style.cssText = prop:value`:

| значение | получено | ожидается |
|---|---|---|
| `z-index:calc(1 + 2)` (position:relative) | `auto` | `3` |
| `order:calc(1 + 2)` | `0` | `3` |
| `opacity:calc(0.5 + 0.25)` | `1` | `0.75` |
| `flex-grow:calc(1 + 2)` | `0` | `3` |
| `transition-delay:calc(1s + 1s)`, `animation-duration:calc(1s + 1s)` | `0s` | `2s` |
| `tab-size:calc(1 + 2)` | `` | `3` |
| `rotate:45deg`, `scale:2`, `translate:5px` (контроль без calc) | `` | `45deg`, `2`, `5px` |
| `transform:rotate(calc(1deg + 1deg))` | `none` | `matrix(…)` |
| `width:calc(100px*pi)`, `calc(100px*e)` | отброшено (ширина авто) | ≈314.16px / 271.83px |
| `width:calc(1px*infinity)`, `calc(100px*NaN)` | отброшено | большая/`calc(NaN*1px)` |
| `width:calc(100px*progress(5px,0px,10px))` | отброшено | `50px` |
| `width:calc-mix(50%,100px,200px)`, `random(100px,100px)` | отброшено | `150px` / `100px` |
| `width:calc(sign(-5px)*-100px)` | `100px` | верно |
| `width:calc(100px*cos(0deg))`, `round(nearest,95px,10px)`, `mod(118px,100px)` | 100px, 100px, 18px | верно |

Setter-проба `el.style[prop] = v; el.style[prop]`: `width='min()'` → `""` (верно), `'clamp()'` → `""`, `'hypot()'` → `""`, но
`zIndex='abs()'` → `"abs()"`, `width='round(1px)'` → `"round(1px)"`, `width='min(1px)'` → `"min(1px)"`
(canonical `calc(1px)`); `calc(1px*NaN)` отвергается (ожидается принять и сериализовать как `calc(NaN * 1px)`).

## Как найдено

WPT-RUN-14 срез 19: `signs-abs-computed` (189/233), `round-mod-rem-computed` (176/243), `round-function` (115/191), `signed-zero`
(162/162), `sin-cos-tan-serialize` (270/270), `acos-asin-atan-atan2-{computed,invalid,serialize}`, `minmax-{angle,integer,number,time}-*`,
`exp-log-*`, `hypot-pow-sqrt-*`, `progress-*`, `calc-infinity-nan-*`, `calc-numbers`, `calc-time-values`, `calc-integer`,
`calc-z-index-fractions-001`, `clamp-*`. Пробой подтверждены строки таблицы; остальное — по первому сообщению сабтеста.

## Что делать

Одно вычисление математических функций (`CalcValue`) с типом результата (`length`/`number`/`integer`/`time`/`angle`/`percentage`),
проверкой совместимости с контекстом свойства и сериализацией в `calc(…)` (CSS Values 4 §10.9); константы и `progress()`/`random()`.
`rotate`/`scale`/`translate` — отдельная причина: computed вообще не возвращается ([CSS-SPECS.md](../CSS-SPECS.md) помечает ✅ только
рендер).

## Как проверить

`css/css-values/signs-abs-computed.html`, `round-mod-rem-computed.html`, `minmax-number-computed.html`, `calc-infinity-nan-computed.html`.
