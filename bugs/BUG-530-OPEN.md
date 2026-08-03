# BUG-530: `Animation.pause()` + `currentTime =` seek never re-applies interpolated styles

**Статус:** OPEN
**Дата:** 2026-08-03
**Компонент:** js (`crates/js/src/dom.rs:15412-15556` — WAAPI `Animation` shim)
**Найден:** P2, WPT-RUN-3 срез 25 (`css/css-properties-values-api`) — живая проба
`--mcp-live-port` на `animation/custom-property-animation-inherited-used-by-standard-property.html`
и аналогах (0/1 сабтест, harness OK)

## Симптом

```js
const anim = target.animate({ marginLeft: ["0px", "100px"] }, 1000);
anim.pause();
anim.currentTime = 250;               // 25% прогресса
getComputedStyle(target).marginLeft;  // ожидание "25px", факт "0px"
```

Подтверждено живой пробой дважды — на стандартном свойстве (`marginLeft`) и на
зарегистрированном custom property (`--my-length` через `CSS.registerProperty` +
`var()`). В обоих случаях после `pause()` + `currentTime =` вычисленный стиль
остаётся на начальном кадре, независимо от `offsetWidth`-флаша (не кэш-гэп
класса BUG-493 — эффект вообще не применён, а не применён-но-не-сброшен).

## Причина

`Animation.prototype._applyAtP` (dom.rs:15548) — единственное место, где
интерполированные стили действительно пишутся на `eff.target.style[prop]` —
вызывается только из `Animation.prototype._tick` (dom.rs:15522), которая сама
запускается только раз-за-разом через `requestAnimationFrame`
(`_scheduleRaf`, dom.rs:15506). `pause()` (dom.rs:15465) явно отменяет RAF
(`_cancelRaf()`) и не вызывает `_applyAtP` сама. Сеттер `currentTime`
(dom.rs:15418-15425) тоже ограничивается бухгалтерией
`_holdTime`/`_startTime` и не вызывает `_applyAtP`. Единственные прямые
вызовы `_applyAtP` вне `_tick` — в `finish()` (dom.rs:15492, `_applyAtP(1)`)
и в ветке fill-mode `_tick` (dom.rs:15541, тоже внутри `_tick`). Итог: пока
анимация `running` (RAF тикает), эффект визуально корректен; как только она
`paused`, любое дальнейшее программное позиционирование через `currentTime =`
— штатный WPT-паттерн для детерминированной выборки прогресса анимации в
тестах — молча не имеет эффекта.

## Влияние

`css/support/interpolation-testcommon.js:230-231` (общий хелпер для *всех*
`*-interpolation.html`/`*-no-interpolation.html` тестов через весь `css/`)
делает ровно `animation.pause(); animation.currentTime = 50 * 1000;` перед
чтением `getComputedStyle()` — то есть каждый файл, уже атрибутированный
[BUG-463](BUG-463-OPEN.md) (`'animate' in Element.prototype` отвечает
`false`, тест падает на feature-detect раньше, чем доходит до этого кода),
после гипотетического фикса BUG-463 **не позеленеет**, а упадёт здесь же —
BUG-463 сейчас маскирует эту находку на десятках файлов через весь корпус
(`css/CSS2`, `css-backgrounds`, `css-logical`, `css-color-adjust`,
`css-content`, `compositing` и другие срезы WPT-RUN-3). Также напрямую бьёт
9 файлов `css/css-properties-values-api/animation/*.html` этого среза
(0/1 сабтест каждый, harness OK, не через `interpolation-testcommon.js` —
собственный inline-паттерн `animate().pause(); .currentTime = ...`).

## Фикс (не сделан)

`currentTime`-сеттер и/или `pause()` должны вызывать `this._applyAtP(p)` с
пересчитанным прогрессом сразу после обновления `_holdTime`/`_startTime`, а
не полагаться на следующий RAF-тик (который во время `paused` никогда не
случится).

## .ini

Не добавлен — находка ещё не привязана к конкретным файлам через `.ini`
(9 файлов `css/css-properties-values-api/animation/` в этом срезе; полный
масштаб — все файлы, уже перечисленные в [BUG-463](BUG-463-OPEN.md), плюс
любые будущие срезы, использующие `interpolation-testcommon.js` или
собственный `pause()+currentTime`-паттерн).
