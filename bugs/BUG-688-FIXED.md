# BUG-688: Touch Events API (`Touch`/`TouchList`/`TouchEvent`, `ontouch*` on `GlobalEventHandlers`) entirely unimplemented

**Статус:** FIXED 2026-09-28 (P3)
**Компонент:** js (`crates/js/src/dom.rs` — `WEB_API_SHIM`; analogous constructors live at `dom.rs:437` `UIEvent`, `dom.rs:445` `MouseEvent`, `dom.rs:536-554` `PointerEvent`, none of that pattern exists for Touch)
**Найден:** P2, WPT-VENDOR-touch-events, 2026-08-06

## Симптом

`touch-events` (скоуп ⬜, кандидат) — вендорена и прогнана целиком
(`run_report.py --all --root touch-events --recursive`, ~53 с, 17 файлов/15 id
после исключения `-manual`/`support`): **8/14 harness OK, 19/44 сабтестов**.

Прямой источник практически всего первичного сигнала — глобальные
конструкторы `Touch`, `TouchList`, `TouchEvent` отсутствуют вовсе, и
`GlobalEventHandlers` (`window`/`HTMLElement.prototype`) не имеет
`ontouchstart`/`ontouchmove`/`ontouchend`/`ontouchcancel`:

```
FAIL Touch::webkitRadiusX - Touch is not defined
ReferenceError: Touch is not defined
FAIL TouchList::identifiedTouch - TouchList is not defined
ReferenceError: TouchList is not defined
FAIL TouchEvent::initTouchEvent - TouchEvent is not defined
ReferenceError: TouchEvent is not defined
FAIL Touch constructor exists and creates a Touch object with minimum properties - Touch is not defined
PRECONDITION_FAILED Touch events in GlobalEventHandlers - 'expose legacy touch event APIs'
```

Confirmed by direct source read: `grep -rni "touch" crates/js/src/*.rs` (excluding
Rust identifiers like `untouched`/`DomTouched`/gamepad's unrelated `touched`
field) has zero hits for `Touch`/`TouchEvent`/`TouchList` as a JS-visible
constructor or interface — the only touch-related JS-visible surface anywhere
in the crate is `sanitizer.rs:27-28`'s allowlist of the `ontouch*` *attribute
names* (for the HTML sanitizer, unrelated to whether the IDL properties
actually exist). No native ever dispatches a `TouchEvent` for a real pointer
gesture either — `single-touch.html`/`single-touch-vertical-rl.html` (whose
sole subtest waits on a real touch dispatched via `test_driver.Actions()`)
TIMEOUT rather than failing an assertion.

Touched files and their direct cause, one run:

- `historical.html` (2/8): `Touch`/`TouchList`/`TouchEvent` `ReferenceError` (4 legacy `webkit*` props + `identifiedTouch` + `initTouchEvent`) — this bug.
- `touch-touchevent-constructor.html` (0/5): same `Touch is not defined` — this bug.
- `touch-globaleventhandler-interface.html` (1/3): `PRECONDITION_FAILED` on `'ontouchstart' in window` / `GlobalEventHandlers.prototype.hasOwnProperty('ontouchstart')` — this bug.
- `expose-legacy-touch-event-apis.html` (16/16 subtests actually pass, but harness itself ERRORs on "4 duplicate test names: `'ontouchstart' in [object Object]`" — the four probed objects (`window`, `document`, `HTMLElement.prototype`, `SVGElement.prototype`) all stringify to the generic `[object Object]` instead of distinct tags, a symptom of the already-open [BUG-367](BUG-367-FIXED.md) point (4)/toStringTag class, not a new finding, and orthogonal to the Touch gap itself since the *values* being compared are consistent).
- `pinch-zoom-change.html` (0/1): `TypeError: Cannot read properties of undefined (reading 'scale')` — the test reads `event.scale` off a dispatched touch/gesture event that Lumen never constructs; same root cause.
- `single-touch.html`, `single-touch-vertical-rl.html` (TIMEOUT): wait on a real dispatched `touchstart`/`touchmove`/`touchend` synthesized via `test_driver.Actions().addPointer({pointerType:'touch'})` — never fires because there is no `TouchEvent` machinery to dispatch in the first place.
- `multi-touch-touchmove.html` (0/1): `action 'action_sequence' not implemented by Lumen's minimal WPT executor` — separate, already-documented test-infra limitation (`tools/wptrunner/wptrunner/executors/executorlumen.py::_handle_action` implements only `action == "click"`), not this bug; listed for completeness since it sits in the same file set.
- `idlharness.window.html` (TIMEOUT): `/resources/idlharness.js` + `/resources/WebIDLParser.js` 404 — unvendored out-of-category dependency, the standard documented survey gap, not an engine bug.

## Вторичный сигнал (реконфирмация, не новое)

- `hover-state-caused-by-compatibility-mouse-events.tentative.html` (0/2),
  `mouseevents-after-touchend.tentative.html` (0/7),
  `multi-touch-interactions.html`/`multi-touch-interfaces.html` (TIMEOUT),
  `single-tap-when-touchend-listener-use-sync-xhr.html` (0/1): all
  `Error: Browsing context for element was detached` — thrown by
  `tools/wptrunner/wptrunner/testdriver-extra.js:118`'s
  `get_context(element)` when `element.ownerDocument.defaultView` is falsy —
  reconfirmation of [BUG-622](BUG-622-DUPLICATE.md) (`document.defaultView`
  missing entirely), not a new finding.

## Как воспроизвести

```
tests/wpt/run_report.py --binary <lumen.exe> --all --root touch-events --recursive
```

или живая проба: `eval("[typeof window.Touch, typeof window.TouchList, typeof window.TouchEvent, 'ontouchstart' in window]")`
→ `["undefined","undefined","undefined",false]` на любой странице.

## Возможный фикс

Add `Touch`/`TouchList`/`TouchEvent` constructors to `WEB_API_SHIM` following
the existing `UIEvent`/`MouseEvent`/`PointerEvent` pattern (`dom.rs:437-554`):
`TouchEvent extends UIEvent` with `touches`/`targetTouches`/`changedTouches`
(each a `TouchList`-like array-like with `item()`/`length`), `Touch` a plain
data holder (`identifier`/`target`/`clientX/Y`/`pageX/Y`/`screenX/Y`/`radiusX/Y`/`rotationAngle`/`force`
— including the legacy `webkit*` aliases `historical.html` probes), plus
`ontouchstart`/`ontouchmove`/`ontouchend`/`ontouchcancel` own-properties on
`GlobalEventHandlers`. Actually dispatching real `TouchEvent`s from a
synthesized touch gesture (shell input layer) is a separate, larger piece of
work than the constructors alone — the constructors fix `historical.html`,
`touch-touchevent-constructor.html`, and the `PRECONDITION_FAILED` half of
`touch-globaleventhandler-interface.html` on their own; dispatch is needed for
`single-touch*.html` and the multi-touch interaction tests.

## Исправление (2026-09-28, P3)

`crates/js/src/shim/web_api_shim_mid.js`, блок сразу после `PointerEvent`:

- `Touch` — конструктор по Touch Events L2 §5.1: `TouchInit` конвертируется в
  WebIDL-порядке членов, `identifier`/`target` обязательны, `target` обязан быть
  `EventTarget` (`location` → `TypeError`), `double`/`float` без не-конечных
  значений, `touchType` — enum `direct`/`stylus`. Никаких `webkit*`-алиасов.
- `TouchList` — не конструируемый; индексные собственные свойства, `length`,
  `item()` (за пределами — `null`), `@@iterator` = `%Array.prototype.values%`.
- `TouchEvent : UIEvent` — `touches`/`targetTouches`/`changedTouches` из
  `sequence<Touch>` (не-`Touch` → `TypeError`), четыре модификатора,
  `getModifierState()`; без `initTouchEvent`.
- Форма по idlharness: атрибуты — геттеры на прототипе с brand-check через
  WeakMap, `Symbol.toStringTag`, нередактируемый `prototype`, неперечислимые
  глобалы.

**`ontouch*` намеренно НЕ выставлены** ни на `window`/`document`, ни на элементах:
спека добавляет их только под «expose legacy touch event APIs», который десктоп
без тачскрина держит выключенным (так же ведёт себя Edge). До фикса
`'ontouchstart' in el` был `true` на элементах (они стояли в
`_LUMEN_EVENT_HANDLER_ATTRS`) при `false` у `window`/`document` — сайты, детектящие
тач по `document.documentElement`, переключались на тач-интерфейс. Контент-атрибут
`ontouchstart="…"` по-прежнему компилируется общим `on*`-путём.
`touch-globaleventhandler-interface.html` поэтому остаётся PRECONDITION_FAILED, а
idlharness — FAIL на `ontouch*`-членах: это ожидаемое десктопное поведение.

Регрессия: `crates/js/src/dom/tests/v8_bug688_touch_events.rs` (4 теста).

WPT `touch-events` (`--all --root touch-events --recursive`, baseline переснят):
**6/14 harness OK, 130/172 сабтестов** (было 8/14, 19/44). `historical.html` 8/8,
`touch-touchevent-constructor.html` 5/5, idlharness теперь исполняется (раньше
падал на `idl_test setup`). Минус два harness OK — не эта правка: с 2026-09-04
исполнитель дошёл до `test_driver.Actions`, и селектор `*|body`, который он строит
для элемента, отвергается `querySelector` ([BUG-1063](BUG-1063-FIXED.md)).

**Остаток вне этого бага:** реальные касания не диспатчатся — шелл не синтезирует
`touchstart`/`touchmove`/`touchend`, а `executorlumen.py` не переводит
`pointerType: "touch"` в касания (`single-touch*.html`, `multi-touch-*.html`,
`pinch-zoom-change.html`).
