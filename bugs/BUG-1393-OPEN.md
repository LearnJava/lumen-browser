# BUG-1393 — `-webkit-transform`, `-webkit-box-shadow`, `-webkit-border-radius`, `-webkit-transition`, `-webkit-box-sizing`, `-webkit-animation`, `-webkit-flex` не распознаются в таблице стилей

**Статус:** OPEN
**Заведён:** 2026-10-07 (P2, WPT-RUN-14 срез 16, `css/css-overflow` + `css/css-sizing`)
**Область:** layout (`crates/engine/layout/src/style/apply/*.rs` — таблица имён свойств)

## Симптом

Страница `<div style="-webkit-transform:translateX(50px)">`, `-webkit-box-shadow:0 0 0 5px red`, `-webkit-border-radius:10px`, `-webkit-transition:width 1s`, `-webkit-box-sizing:border-box;padding:5px;width:50px`, `-webkit-animation-name:x`, `-webkit-flex:1`; `getComputedStyle`: `transform:none`, `boxShadow:none`, `borderTopLeftRadius:0px`, `transitionProperty:all`, `boxSizing:content-box`, `animationName:none`, `flexGrow:""`. `CSS.supports('-webkit-transform','none')` и ещё шесть таких — `false`; `-webkit-appearance`, `-webkit-user-select` — `true`.

Compatibility Standard §3 требует псевдонимов именно для этих имён (Chrome/Safari/Firefox держат их все).

## Как найдено

Побочная находка WPT-RUN-14 среза 16 при пробе BUG-1392; **в id этого среза не измерена** (в `css-overflow`/`css-sizing` таких тестов нет), число id по `css` не считалось.

## Что делать

Таблица алиасов `-webkit-X` → `X` для перечисленных свойств (как уже сделано для `-webkit-user-select`, `-webkit-appearance`, `-webkit-text-size-adjust`).

## Как проверить

Страница выше; `css/css-transforms/` и `css/css-backgrounds/` — прогон без изменений числа, пока в наборе нет тестов с этими алиасами.
