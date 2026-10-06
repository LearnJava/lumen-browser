# BUG-1293 — CSS-переходы и анимации стартуют на следующем кадре: синхронное чтение `getComputedStyle()`/`getAnimations()` видит конечное значение

**Статус:** OPEN
**Заведён:** 2026-10-06 (P2, WPT-RUN-14 срез 5, `css/css-transforms`)
**Область:** layout/shell (`TransitionScheduler`/`AnimationScheduler` создают переход/анимацию при тике кадра, а не при пересчёте стиля); связано с [BUG-536](BUG-536-FIXED.md), [GAP-CSSANIM](../ROADMAP.md)

## Симптом

`--dump-layout`, один скрипт, без ожидания кадра — ровно как делает `interpolation-testcommon.js`:

```js
d.style.opacity = '0'; getComputedStyle(d).opacity;           // 0
d.style.transition = 'opacity 100s -50s linear';
d.style.opacity = '1';
getComputedStyle(d).opacity;   // 1, ожидается 0.5
d.getAnimations().length;      // 0, ожидается 1
```

То же для `transform` (`matrix(1,0,0,1,100,0)` вместо `…,50,0)`), `margin-left`, `perspective-origin`, `color` (`rgb(100,100,100)`
вместо 50) — любое значение сразу конечное. Через `@keyframes` с `animation-delay: -50s` — `none`/начальное значение.
Для **Web Animations** тот же сценарий верен: `el.animate([...]).pause(); currentTime = 25` даёт `0.25`/`matrix(…,25,0)`,
`composite: 'add'` тоже верен — то есть интерполятор исправен, не хватает момента старта.

Что происходит после кадра, headless-проба не показывает (`--dump-layout` не крутит кадры и таймеры) — не проверялось.

## Как найдено

WPT-RUN-14 срез 5: `css/css-transforms/animation/*` — 15 файлов, 1681 упавший сабтест (`transform-interpolation-00{1…6}`,
`perspective-origin-interpolation`, `list-interpolation`, `transform-interpolation-computed-value`, …) в режимах
`CSS Transitions`, `CSS Transitions with transition: all`, `CSS Animations`; режим `Web Animations` падает заметно реже
(260 из 2526). Из них 1681 — крупнейший кластер модуля.

## Что делать

Создавать переход при смене computed-значения (CSS Transitions L1 §3 — «style change event», момент — пересчёт стиля, который
`getComputedStyle()` и `getAnimations()` обязаны форсировать), а анимацию из `animation-name` — при том же пересчёте;
значение на момент времени 0 + `delay`. `GAP-CSSANIM` закрыт, но его срезы добавляли публикацию значения в снэпшот раз в кадр.

## Как проверить

`css/css-transforms/animation/transform-interpolation-00{1…7}.html`, `list-interpolation.html`; общий харнесс
`css/support/interpolation-testcommon.js` подключают 337 файлов `css/` (`css-transitions`, `css-animations`, `css-values`…).
