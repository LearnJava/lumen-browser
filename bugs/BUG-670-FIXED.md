# BUG-670 — `AnimationEffect.getComputedTiming()` missing entirely (only `getTiming()` exists)

**Статус:** FIXED 2026-09-26 (P3)
**Компонент:** js (`crates/js/src/shim/web_api_shim_tail_b.js` — Web Animations, `KeyframeEffect.prototype`; в момент заведения код жил в `dom.rs:12449-12452`)
**Найден:** P2, WPT-VENDOR-scroll-animations, 2026-08-06

## Симптом

Категория `scroll-animations` (`tests/wpt/scroll-animations/`, 280 файлов) — вендорена
и прогнана целиком (`run_report.py --all --root scroll-animations --recursive`,
~2:46, 205 отобранных id): 160/205 harness OK, 355/1853 сабтестов. Большая часть
провалов — уже задокументированный класс «внекатегорийный хелпер не вендорен»
(`/web-animations/testcommon.js` даёт `ReferenceError: createDiv/target/scroller/
container/... is not defined` на ~460+ сайтах вызова, `/dom/events/scrolling/
scroll_support.js` аналогично; та же методология, что `FileAPI`/`animation-worklet`
— см. `docs/wpt-status.md`, не вендорится намеренно).

Отдельно от этого класса — 40× `TypeError: CSS.percent is not a function` и
6× `TypeError: CSS.px is not a function`: уже покрыто открытым
[BUG-554](BUG-554-FIXED.md) (CSS Typed OM numeric factory functions отсутствуют
целиком), не новая находка.

Но 2× `TypeError: animation.effect.getComputedTiming is not a function`
(`scroll-timelines/intrinsic-iteration-duration.tentative.html`,
`view-timelines/zero-intrinsic-iteration-duration.tentative.html`) — это код,
вызванный напрямую тестом на живом `KeyframeEffect`/`Animation`, созданными
самим тестом (не зависит от невендоренного `testcommon.js`), и не покрыто
ни одним открытым тикетом (`BUG-536` — про CSS Transitions, другой механизм).
Живая проба (`--mcp-live-port`) подтверждает вне зависимости от WPT-раннера:

```json
{"getTiming_typeof": "function", "getComputedTiming_typeof": "undefined",
 "anim_effect_getComputedTiming": "undefined"}
```

`kf.getTiming` существует и работает; `kf.getComputedTiming` и
`animation.effect.getComputedTiming` — оба `undefined`.

## Причина

`crates/js/src/dom.rs:12449-12452` определяет на `KeyframeEffect.prototype`
только четыре метода:

```js
KeyframeEffect.prototype.getTiming    = function() { return Object.assign({}, this._timing); };
KeyframeEffect.prototype.updateTiming = function(t) { Object.assign(this._timing, t); };
KeyframeEffect.prototype.getKeyframes = function() { return this._keyframes.slice(); };
KeyframeEffect.prototype.setKeyframes = function(kf) { this._keyframes = _wa_normalize_keyframes(kf); };
```

Спека (Web Animations §5.4, `AnimationEffect` interface, наследуемый
`KeyframeEffect`) требует отдельный `getComputedTiming()` — не алиас
`getTiming()`, а метод, возвращающий *разрешённые* (computed) значения:
`duration: 'auto'` → фактическая длительность в мс, `fill: 'auto'` →
разрешается в `'none'` (кроме CSS-анимаций/переходов), плюс вычисляемые
поля, которых у `getTiming()` нет вовсе — `localTime`, `progress`,
`currentIteration`, `activeDuration`, `endTime`. `getTiming()` — это
*specified* timing (что передал пользователь), `getComputedTiming()` —
*computed* timing (что из этого вышло); шим реализует только первое,
второй метод отсутствует как таковой, а не просто некорректен.

## Масштаб

Затрагивает любой код, использующий `effect.getComputedTiming()` —
стандартный способ прочитать прогресс/фазу анимации извне (в т.ч. Scroll-
/View-timeline тесты этой категории, которые опрашивают computed timing,
чтобы проверить резолюцию `duration: 'auto'` от scroll-driven таймлайна).
В этой WPT-категории — 18 файлов ссылаются на `getComputedTiming` (см.
`grep -rl getComputedTiming tests/wpt/scroll-animations/`), но большинство
из них уже блокируются на невендоренном `testcommon.js` раньше, чем
доходят до вызова; только 2 реально исполнились и провалились именно на
этом.

## Дальше

Fix scope: добавить `KeyframeEffect.prototype.getComputedTiming`,
резолвящий `_timing` в вычисленные значения (как минимум `duration`/`fill`
auto-резолюция + `endTime`/`activeDuration`; `localTime`/`progress`/
`currentIteration` требуют доступа к текущему времени владеющего
`Animation`, которого `KeyframeEffect` сам по себе не имеет — see
`this.target`/родительский `Animation` через back-reference, если он
существует в шиме). Вне скоупа этой WPT-VENDOR-задачи (только вендоринг +
прогон + живая проба).

## Исправление (2026-09-26, P3)

`KeyframeEffect.prototype.getComputedTiming` (неперечисляемый, как прочие IDL-члены)
в [`web_api_shim_tail_b.js`](../crates/js/src/shim/web_api_shim_tail_b.js) считает
computed timing по Web Animations §4.6–4.10: `duration: 'auto'` → 0, `fill: 'auto'` →
`'none'`, `activeDuration`, `endTime`; `localTime` — `currentTime` владеющей анимации
(конструктор `Animation` ставит эффекту неперечисляемую обратную ссылку `_animation`),
по нему фаза before/active/after, active time, overall/simple progress,
`currentIteration`, направление и easing → `progress`. У эффекта без анимации три
временных поля — `null`.

Попутно: анимация нулевой длительности после задержки теперь финиширует
(`_wa_iter_progress` возвращал `1` вместо «после конца», и `finished` не резолвился
никогда) — без этого WPT `current-iteration.html` уходил в TIMEOUT, дойдя до ожидания.

Тесты — `get_computed_timing_*` и `zero_duration_animation_finishes_after_delay` в
`crates/js/src/dom/tests/v8_window_anim_compress.rs`.

**WPT `web-animations` A/B** (`run_report.py --all --root web-animations --recursive`,
тот же слот, до/после): 123/139 harness OK → 123/139, **907 → 1108/3034 сабтестов**.
`getComputedTiming.html` 0→36/36, `simple-iteration-progress.html` 0→49/49,
`current-iteration.html` 0→51/51, `phases-and-states.html` 0→11/11,
`updateTiming.html` 9→40/68, `active-time.html` 0→8/14, `transformed-progress.html` 0→5/33.

## Остаток

- `fill: forwards|both` не доходит до `finished`, а отрисовка в фазе after игнорирует
  `iterations`/`direction` — [BUG-1192](BUG-1192-OPEN.md) (4 провала `active-time.html`).
- Нет интерфейса `AnimationEffect` (`KeyframeEffect` наследует прямо от `Object`).
- `duration: 'auto'` у scroll-/view-таймлайна должен разрешаться в процентную
  intrinsic-длительность (CSSNumberish); шим не моделирует её, поэтому
  `scroll-timelines/intrinsic-iteration-duration.tentative.html` и
  `view-timelines/zero-intrinsic-iteration-duration.tentative.html` теперь проходят
  дальше `TypeError`, но падают на `assert_percents_equal` (домен
  [BUG-127](BUG-127-OPEN.md), scroll-driven animations).
