# BUG-670 — `AnimationEffect.getComputedTiming()` missing entirely (only `getTiming()` exists)

**Статус:** OPEN
**Компонент:** js (`crates/js/src/dom.rs:12449-12452`, Web Animations `WEB_API_SHIM` — `KeyframeEffect.prototype`)
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
[BUG-554](BUG-554-OPEN.md) (CSS Typed OM numeric factory functions отсутствуют
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
