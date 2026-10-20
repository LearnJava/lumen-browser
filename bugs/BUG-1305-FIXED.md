# BUG-1305 — Web Animations: неявный («neutral») ключевой кадр, шорткод `border-width`, `background-size` и `box-shadow` не интерполируются

**Статус:** FIXED 2026-10-20 (P3)
**Заведён:** 2026-10-06 (P2, WPT-RUN-14 срез 6, `css/css-backgrounds`)
**Область:** js shim (`crates/js/src/shim/web_api_shim_tail_b.js` — `_wa_compute_at_p`, `_wa_interp_prop`; `el.animate()` живёт в шиме, а не в `animation.rs`; связано с [BUG-1234](BUG-1234-OPEN.md), у которого перечень свойств шире)

## Симптом

`el.animate(kf, {duration: 100, fill: 'both', easing: 'linear'}); a.pause(); a.currentTime = 30; getComputedStyle(el)[p]` (`--dump-layout`, один скрипт):

| ключевые кадры | свойство | получено | ожидается |
|---|---|---|---|
| `[{backgroundPositionX:'0px'}→{…:'100px'}]` | `backgroundPositionX` | `30px` | верно |
| `[{borderTopLeftRadius:'0px'}→{…:'100px'}]`, `[{borderRadius:'20px'}→{…:'30px'}]` | радиус | `30px`, `23px` | верно |
| `[{borderTopWidth:'0px'}→{…:'100px'}]` | `borderTopWidth` | `30px` | верно |
| `[{backgroundColor:'rgb(0,0,0)'}→{…:'rgb(100,100,100)'}]` | цвет | `rgb(30,30,30)` | верно |
| **один кадр** `[{backgroundColor:'rgb(100,100,100)'}]` при `background-color:rgb(0,0,0)` на элементе | `backgroundColor` | `rgb(100,100,100)` — сразу конечное | `rgb(30,30,30)`: недостающий начальный кадр берётся из вычисленного значения элемента («neutral») |
| то же, `[{backgroundPositionX:'80px'}]` при `0px` на элементе | `backgroundPositionX` | `80px` | `24px` |
| `[{borderWidth:'20px 40px 60px 80px'}→{…:'30px 50px 70px 90px'}]` (шорткод) | `borderTopWidth` | `20px` — не меняется | `23px` |
| `[{backgroundSize:'10px 10px'}→{…:'20px 20px'}]` | `backgroundSize` | `10px 10px` | `13px 13px` |
| `[{boxShadow:'0 0 0 0 #000'}→{…:'100px 0 0 0 #000'}]` | `boxShadow` | `0px 0px 0px 0px` | `30px 0px 0px 0px` |

## Как найдено

WPT-RUN-14 срез 6: режим `Web Animations` — 266 упавших сабтестов в `css/css-backgrounds/animations/*-interpolation.html`: `box-shadow` 59, `border-width` 45, `background-size` 30, `border-radius` 28 (шорткод `border-radius` в тестах — сложная форма с `/`; проба выше — простая, верная), `background-color` 27 («from neutral to [green]»), `background-position-x/y` по 23, `border-color` 21, `discrete` 7, `background-image` 3. Режимы `CSS Transitions`/`CSS Animations` у тех же файлов падают ещё и по [BUG-1293](BUG-1293-FIXED.md) (старт на следующем кадре) — отделить вклад BUG-1305 можно только после его исправления.

## Что делать

(1) Неявный начальный/конечный кадр — из вычисленного значения элемента (Web Animations 1 §5.4.4 «computing keyframe offsets / neutral keyframe»). (2) Шорткоды (`border-width`, `border-color`, `border-radius` в полной форме) раскрывать до longhand'ов перед интерполяцией. (3) Интерполяция/сложение `<bg-size>` (поэлементно по слоям, `auto` — дискретно) и `<shadow>` (список, `inset` должен совпадать, недостающие слои — прозрачная нулевая тень; CSS Backgrounds 3 §7.2).

## Как проверить

`css/css-backgrounds/animations/{background-color,background-position-x,background-size,border-width,border-color,box-shadow}-interpolation.html`; проба в таблице выше.

## Дополнение: WPT-RUN-14 срез 9 (2026-10-06, `css/css-text`, часть 1)

`css/css-text/animations/{letter-spacing,word-spacing,text-indent}-interpolation.html` — по 20 сабтестов «from neutral» (+ `inherit`/`unset`/`initial`/`normal` по 19–20). Проба на `letter-spacing`: элемент с `letter-spacing: 10px`, `animate([{letterSpacing:'20px'}], …)` на 50 % читает `20px` (ожидается `15px`); двухкадровая `0px → 20px` на 50 % — `10px` (верно); `composite: 'add'` поверх `100px` — `120px`/`110px` (верно). То есть сломан неявный кадр, а не сложение. `letter-spacing-composition.html`/`word-spacing-composition.html`/`text-indent-composition.html` (CSS Animations + `animation-composition`, 15–31 сабтест) читают базовое значение `50px` вместо сложенного — этот путь пробой не проверялся. Всего 6 id / 396 сабтестов; ещё 2 id / 180 сабтестов (`hyphen-limit-chars-interpolation`, `hyphen-no-interpolation`) — самих свойств `hyphenate-*` нет (CSS-SPECS.md:76).

## Исправление (2026-10-20, P3)

Web Animations из JS (`el.animate`) считаются в шиме `web_api_shim_tail_b.js`, не в `animation.rs`:

- `_wa_compute_at_p` теперь ведёт каждое свойство по **своим** ключевым кадрам (Web Animations 1 §5.4.3) и достраивает отсутствующий 0 %/100 % «neutral»-кадром из вычисленного значения элемента (`_wa_capture_underlying` читает `getComputedStyle` для свойств, у которых край не назван; `_wa_neutral_props`). Исключены `transform`/`translate`/`rotate`/`scale` — у шимового интерполятора нет формы для вычисленной `matrix()`.
- Единственный кадр без `offset` получает offset 1 (а не 0), как в спеке.
- Шорткоды `border-width/-color/-style/-radius` (в т.ч. форма с `/`), `margin`, `padding` раскрываются в longhand'ы до интерполяции (`_wa_expand_shorthand`).
- `background-size` (`_wa_lerp_bg_size`, списки до НОК, `auto` против длины — дискретно) и `box-shadow`/`text-shadow` (`_wa_lerp_shadow_list`, §7.2: добор прозрачной нулевой тенью, `inset` обязан совпасть, цвет в premultiplied sRGB); `normal` у `letter-spacing`/`word-spacing` = `0px`; именованные цвета и `hsl()` — через нативный `_lumen_css_color_rgba`.
- Тесты: `crates/js/src/dom/tests/v8_bug1305_wa_neutral.rs`; проба `--dump-layout` на живом движке: `backgroundPositionX` 24px, `borderTopWidth` 23px, `backgroundSize` 13px 13px, `boxShadow` `rgb(0, 0, 0) 30px 0px 0px 0px`, `letterSpacing` 13px, `borderTopColor` `rgb(0, 38, 0)`.

**Остаток (не в этом баге).** WPT-прогон `css/css-backgrounds/animations/*` не делался. Режимы `CSS Animations`/`CSS Transitions` идут через `animation.rs` и JS-overlay BUG-1293, а не через `_wa_compute_at_p`; смесь `px` и `%` (`background-position-x: 0%` → `80px`) по-прежнему дискретна (нужен `calc()`); неявный кадр для `transform` не реализован.
