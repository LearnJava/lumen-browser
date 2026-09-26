# BUG-1192 — анимация с `fill: forwards`/`both` не доходит до `finished`

**Статус:** OPEN
**Заведён:** 2026-09-26 (P3, при закрытии [BUG-670](BUG-670-FIXED.md)).
**Область:** js — Web Animations в
[`web_api_shim_tail_b.js`](../crates/js/src/shim/web_api_shim_tail_b.js)
(`_wa_iter_progress`, `Animation.prototype._tick`).

## Симптом

```js
var a = el.animate({opacity: [0, 1]}, {duration: 100, fill: 'forwards'});
// через 100+ мс:
a.playState      // 'running' навсегда, ожидается 'finished'
a.finished       // не резолвится, onfinish/событие finish не приходят
```

С `fill: 'none'`/`'auto'` та же анимация финиширует нормально. Воспроизведено
юнит-пробой (`_wa_current_time = 500; a._tick(500); a.playState` → `running`).

## Механизм

`_wa_iter_progress` после конца активного интервала возвращает `1` при
`fill: forwards|both` и `-2` («после конца, не в эффекте») иначе. `_tick`
переводит анимацию в `finished` только по `-2`; на `1` он применяет последний
кадр и заново планирует RAF — бесконечно. Фаза («после конца») и то, что
рисовать (fill), смешаны в одном числе.

## Что требуется

Разделить фазу и прогресс: `_tick` должен финишировать по концу активного
интервала независимо от `fill`, а `fill` решать только, какой кадр остаётся.
Подводные камни, из-за которых это не сделано вместе с BUG-670:

- `_tick` на финише вынимает анимацию из `_wa_animations`, а finished-анимация
  с `fill: forwards` по спеке остаётся «relevant» и должна быть в
  `getAnimations()`;
- став `finished`, такие анимации начнут проходить `_wa_process_replacements`,
  а `_wa_remove_replaced` зовёт `_clearStyles()` у старой анимации — это
  стирает inline-стили тех же свойств, уже записанные новой; нужен порядок
  или композиция, иначе видимый откат стиля.

Проверка — WPT `web-animations/timing-model/animations/finishing-an-animation.html`
и юнит-тест в `crates/js/src/dom/tests/v8_window_anim_compress.rs`.

Тот же корень у 4 провалов WPT `web-animations/timing-model/animation-effects/active-time.html`
(«Active time in after phase with forwards/both fill…», `expected 2 but got 1`):
в фазе after с `fill: forwards` рисуется прогресс `1` без учёта `iterations`/
`direction`/`endDelay`, хотя `getComputedTiming()` (BUG-670) считает фазу верно.
Логично перевести отрисовку на тот же расчёт фаз, что в `getComputedTiming`.
