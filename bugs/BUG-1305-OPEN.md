# BUG-1305 — Web Animations: неявный («neutral») ключевой кадр, шорткод `border-width`, `background-size` и `box-shadow` не интерполируются

**Статус:** OPEN
**Заведён:** 2026-10-06 (P2, WPT-RUN-14 срез 6, `css/css-backgrounds`)
**Область:** layout (`crates/engine/layout/src/animation.rs` — интерполятор свойств и сборка ключевых кадров из `KeyframeEffect`; связано с [BUG-1234](BUG-1234-OPEN.md), у которого перечень свойств шире)

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

WPT-RUN-14 срез 6: режим `Web Animations` — 266 упавших сабтестов в `css/css-backgrounds/animations/*-interpolation.html`: `box-shadow` 59, `border-width` 45, `background-size` 30, `border-radius` 28 (шорткод `border-radius` в тестах — сложная форма с `/`; проба выше — простая, верная), `background-color` 27 («from neutral to [green]»), `background-position-x/y` по 23, `border-color` 21, `discrete` 7, `background-image` 3. Режимы `CSS Transitions`/`CSS Animations` у тех же файлов падают ещё и по [BUG-1293](BUG-1293-OPEN.md) (старт на следующем кадре) — отделить вклад BUG-1305 можно только после его исправления.

## Что делать

(1) Неявный начальный/конечный кадр — из вычисленного значения элемента (Web Animations 1 §5.4.4 «computing keyframe offsets / neutral keyframe»). (2) Шорткоды (`border-width`, `border-color`, `border-radius` в полной форме) раскрывать до longhand'ов перед интерполяцией. (3) Интерполяция/сложение `<bg-size>` (поэлементно по слоям, `auto` — дискретно) и `<shadow>` (список, `inset` должен совпадать, недостающие слои — прозрачная нулевая тень; CSS Backgrounds 3 §7.2).

## Как проверить

`css/css-backgrounds/animations/{background-color,background-position-x,background-size,border-width,border-color,box-shadow}-interpolation.html`; проба в таблице выше.
