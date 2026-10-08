# BUG-668 — `screen.orientation` doesn't inherit `EventTarget` and `.lock()` has zero effect on reported orientation

**Статус:** FIXED 2026-09-17 (P3)
**Компонент:** js (`crates/js/src/screen_orientation.rs` — `SCREEN_ORIENTATION_SHIM`, Phase 0 Screen Orientation stub)
**Найден:** P2, WPT-VENDOR-screen-orientation (2026-08-06), live `--mcp-live-port` probe (the WPT run itself gave zero functional signal — all 13 module-importing test files TIMEOUT on the already-documented [BUG-446](BUG-446-FIXED.md) module-graph gap, `idlharness.window.html` TIMEOUT on the already-documented recurring idlharness infra gap, `lock-bad-argument.html` — the one file that imports nothing — is the only one that ran, 2/2 subtests OK)

## Run signal

```
tests: 1/15 harness OK; subtests: 2/2 passed
```

Every functional test (`active-lock.html`, `lock-basic.html`, `lock-unlock-check.html`,
`onchange-event.html`, `onchange-event-subframe.html`, `orientation-reading.html`,
`nested-documents.html`, `non-fully-active.html`, `unlock.html`, `event-before-promise.html`,
`fullscreen-interactions.html`, `lock-sandboxed-iframe.html`, `hidden_document.html`) imports
shared helpers via `import {...} from "./resources/orientation-utils.js"` — this reconfirms
BUG-446 (the shell never loads a `<script type="module">`'s imported modules, so `import_test`
never registers a single `test()` call and the harness TIMEOUTs with zero subtests) rather than
finding anything new. `idlharness.window.html` reconfirms the recurring, already-documented
idlharness infra gap (`/resources/WebIDLParser.js` + `/resources/idlharness.js` 404, not
vendored). Since the module-loading gate blocks 100% of the functional signal this category
would otherwise produce, a direct `--mcp-live-port` probe of the live `screen.orientation`
object was run instead (same "probe when the run gives nothing" convention as
[BUG-666](BUG-666-FIXED.md)/[BUG-667](BUG-667-FIXED.md)).

## Probe and result

```js
screen.orientation instanceof EventTarget        // → false
typeof screen.orientation.dispatchEvent          // → "undefined"
typeof screen.orientation.addEventListener       // → "function" (own-instance method, not inherited)
Object.getPrototypeOf(Object.getPrototypeOf(screen.orientation)) === EventTarget.prototype
                                                  // → false

screen.orientation.lock('landscape-primary').then(() => ({
  type: screen.orientation.type,   // → "portrait-primary" (unchanged)
  angle: screen.orientation.angle  // → 0 (unchanged)
}))
```

Two independent defects in `SCREEN_ORIENTATION_SHIM` (`screen_orientation.rs:20`-`127`):

1. **`ScreenOrientation` does not inherit `EventTarget`** (spec: `interface ScreenOrientation :
   EventTarget`, W3C Screen Orientation §4). The shim (`screen_orientation.rs:37`-`45`) is a
   bare constructor function with its own hand-rolled `_listeners` object and
   `addEventListener`/`removeEventListener` methods (lines 95-103) instead of
   `Object.create(EventTarget.prototype)` — so `instanceof EventTarget` is `false` and
   `dispatchEvent` is entirely missing (not merely broken: `typeof ... === "undefined"`). Any
   test or page script that dispatches a synthetic `change` event via the standard
   `dispatchEvent(new Event('change'))` path (rather than calling the internal
   `_fireChangeEvent` the shim itself defines) cannot work at all. Same class of defect as
   [BUG-664](BUG-664-FIXED.md) (`navigator.connection` not an `EventTarget`) and
   [BUG-400](BUG-400-FIXED.md) (`performance` a plain object literal) — a recurring pattern
   of hand-rolled pub/sub standing in for real `EventTarget` inheritance across Phase 0 shims.

2. **`.lock()` resolves successfully but never updates `type`/`angle`.** `lock()`
   (`screen_orientation.rs:50`-`79`) validates the orientation string against the spec's enum,
   sets `self._lockOrientation` (an internal field nothing else reads), optionally calls the
   native `_lumen_set_fullscreen` binding, and resolves with `self` — but never assigns
   `self.type`/`self.angle`, and never calls its own `_fireChangeEvent`. Confirmed live:
   `screen.orientation.lock('landscape-primary')` resolves (no rejection), yet
   `screen.orientation.type` stays `'portrait-primary'` and `.angle` stays `0` afterward — a
   *successful* lock is observationally identical to a no-op. This independently breaks every
   test that checks the post-lock orientation state (`active-lock.html`, `lock-basic.html`,
   `lock-unlock-check.html`, `orientation-reading.html`) — even setting BUG-446 aside, none of
   them would pass once the module-loading gate is fixed, because the API underneath does
   nothing observable on success.

## Что НЕ является причиной этого бага

- The 13-file module-import TIMEOUT wall — pure reconfirmation of BUG-446, not re-filed here.
- `idlharness.window.html`'s TIMEOUT — pure reconfirmation of the recurring, already-documented
  idlharness infra gap (unvendored common `WebIDLParser.js`/`idlharness.js`).
- `lock-bad-argument.html` passing 2/2 — the one test that doesn't need module import or an
  actual state change (it only checks that invalid orientation strings reject); this part of
  the shim (the enum validation at the top of `lock()`) is correct.

## Предлагаемый фикс

1. Rebuild `ScreenOrientation`/`ScreenOrientationEvent` on top of the shim's real `EventTarget`
   base (whatever `dom.rs`'s `WEB_API_SHIM` already exposes for other event-emitting globals —
   grep for `Object.create(EventTarget.prototype)` for the established pattern) instead of the
   private `_listeners` array, so `dispatchEvent`/`instanceof` work per spec; keep
   `_fireChangeEvent` as the internal trigger but have it call `this.dispatchEvent(evt)` rather
   than manually iterating `_listeners`.
2. Have `lock()` actually assign `self.type`/`self.angle` to the requested orientation (resolved
   to a concrete non-`'any'`/non-bare value, e.g. `'landscape'` → `'landscape-primary'`) before
   resolving, and fire the `change` event on that assignment — matching the Phase 0 scope note
   already in the file's doc comment ("orientation type is static 'portrait-primary' for now")
   which should be corrected once this lands, since a fixed `lock()` makes the type non-static.

Owner — P1/P3 (js). Both fixes are self-contained to `screen_orientation.rs` and its unit test
module; no shell/native binding changes required for either.

## Исправлено

`ScreenOrientation` перестроен на реальном `EventTarget` — `EventTarget.call(this)` в
конструкторе плюс `ScreenOrientation.prototype = Object.create(EventTarget.prototype)`,
тот же паттерн, что уже используют `VisualViewport`/`Animation`/`EventSource` в
`crates/js/src/shim/*.js`. Ручной `_listeners`-массив и собственные
`addEventListener`/`removeEventListener` удалены — теперь оба унаследованы от
`EventTarget.prototype`, `instanceof EventTarget` и `dispatchEvent` работают по спеке.
`_fireChangeEvent` больше не перебирает `_listeners` вручную и не дёргает `onchange`
отдельно — просто зовёт `this.dispatchEvent(evt)`, а `onchange` срабатывает через уже
существующий `this['on' + type]`-лукап внутри `EventTarget.prototype.dispatchEvent`
(`event_target_shim.js`). `ScreenOrientationEvent` теперь строится через
`Event.call(this, type, init)` вместо ручной инициализации полей.

`.lock(orientation)` теперь резолвит запрошенную ориентацию в конкретный тип
(`resolveLockedType`: `'landscape'`/`'portrait'` → `-primary`-вариант, `'any'` оставляет
текущий тип неизменным), назначает `type`/`angle` по таблице `ANGLE_BY_TYPE`
(`portrait-primary`=0, `landscape-primary`=90, и т.д.) и зовёт `_fireChangeEvent` перед
резолвом промиса — успешный `lock()` больше не наблюдательно эквивалентен no-op.

Новые тесты (`crates/js/src/screen_orientation.rs::tests`): `screen_orientation_is_event_target`,
`screen_orientation_dispatch_event_fires_change_listener`, `screen_orientation_lock_updates_type_and_angle`,
`screen_orientation_lock_any_keeps_current_type`. Тестовый хелпер `with_screen_orientation` теперь
поднимает реальный `EVENT_TARGET_SHIM` вместо голого `screen`-объекта.

`cargo test -p lumen-js --lib screen_orientation --features v8-backend` 13/13,
`cargo clippy --workspace --all-targets -- -D warnings` чист. `scripts/scoped-test.sh` — единственный
красный тест (`cases::snapshot_cpu::cpu_snapshots_match_references`, те же 7 файлов) совпадает
byte-for-byte с уже задокументированным чужим дрейфом [BUG-1008](BUG-1008-OPEN.md), не регрессия;
`python graphic_tests/dump_golden.py` — 4/12 несовпадений, тот же baseline-дрейф, что уже есть на
`main`. Только JS-шим, пиксели не затронуты; WPT-модульный гэп (BUG-446) и idlharness-инфра-гэп
не в скоупе этого фикса.
