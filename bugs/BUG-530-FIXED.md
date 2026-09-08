# BUG-530: `Animation.pause()` + `currentTime =` seek never re-applies interpolated styles

**Статус:** FIXED (2026-09-08, P3)
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
[BUG-463](BUG-463-FIXED.md) (`'animate' in Element.prototype` отвечает
`false`, тест падает на feature-detect раньше, чем доходит до этого кода),
после гипотетического фикса BUG-463 **не позеленеет**, а упадёт здесь же —
BUG-463 сейчас маскирует эту находку на десятках файлов через весь корпус
(`css/CSS2`, `css-backgrounds`, `css-logical`, `css-color-adjust`,
`css-content`, `compositing` и другие срезы WPT-RUN-3). Также напрямую бьёт
9 файлов `css/css-properties-values-api/animation/*.html` этого среза
(0/1 сабтест каждый, harness OK, не через `interpolation-testcommon.js` —
собственный inline-паттерн `animate().pause(); .currentTime = ...`).

## Фикс (2026-09-08, P3)

Подтверждена ровно гипотеза из раздела «Причина» — правки только в
`crates/js/src/shim/web_api_shim_tail_b.js`, без изменений `_wa_iter_progress`
или семантики `_applyAtP`.

`Animation.prototype._tick` уже вычисляла прогресс `p` и решала, красить ли
кадр (fill-режим для `p === -1`, иначе прямой `_applyAtP(p)`) — эта логика
вынесена в общий хелпер `_applyForIterProgress(p, eff)`, который теперь
разделяют `_tick` (RAF-driven, `running`) и новый
`_syncStyleAtCurrentTime()` (вызывается вне RAF). `_syncStyleAtCurrentTime`
читает `this.currentTime`, прогоняет его через тот же `_wa_iter_progress`,
и дополнительно обрабатывает `-2` («после конца, без fill-forwards») —
кейс, которого не было в `_tick` (RAF туда не доходит: `_onFinish`
перехватывает раньше), но который встречается на прямом `currentTime =`
сике за пределы длительности — как «показать последний кадр» (`_applyAtP(1)`).

`_syncStyleAtCurrentTime()` вызывается из двух мест:
- сеттер `currentTime` (dom.rs / `web_api_shim_tail_b.js`, после обновления
  `_holdTime`/`_startTime`) — синхронно перекрашивает кадр на новую
  позицию, независимо от состояния `running`/`paused`/`idle`;
- `pause()`, сразу после `_cancelRaf()` — красит кадр на момент паузы,
  который иначе никогда не будет нарисован (RAF, который иначе бы это
  сделал, уже отменён этим же вызовом).

Тесты (`crates/js/src/dom/tests/v8_window_anim_compress.rs`):
`animation_pause_then_seek_reapplies_style` (сценарий бага — `pause()` +
`currentTime = 250` на 1000мс `marginLeft`-анимации → `"25px"`),
`animation_pause_reapplies_style_at_current_time` (голый `pause()` без
последующего сика красит кадр на момент паузы).

`cargo test -p lumen-js --lib --features v8-backend`
(`dom::tests::v8_window_anim_compress`): 70/70. `cargo clippy -p lumen-js
--all-targets -- -D warnings`: чисто на затронутых файлах — полный прогон
красит несвязанные `lumen-image`/`lumen-font` (`chunks_exact_to_as_chunks`,
системный rustc 1.98.x вместо пина 1.97.0 на этой машине, см. память
`feedback_linux_toolchain_mismatch`).

## .ini

Не добавлен этим фиксом — находка была атрибутирована по симптому
(`css/support/interpolation-testcommon.js`'s `pause()+currentTime`-паттерн),
не по конкретным `.ini`-файлам; актуализация покрытия — по факту следующего
прогона WPT на затронутых срезах (маскировалось [BUG-463](BUG-463-FIXED.md)
до его фикса 2026-09-01, актуальный масштаб непроверен).
