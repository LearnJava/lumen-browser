# BUG-655 — `requestPointerLock()` не проверяет transient user activation, всегда гарантирует блокировку

**Статус:** OPEN
**Компонент:** js (`crates/js/src/pointer_lock.rs:42-46` — `request_pointer_lock`,
`crates/js/src/v8_runtime.rs:3248-3250` — привязка `_lumen_ptr_lock_request`)
**Найден:** P2, WPT-VENDOR-pointerlock (2026-08-05), прогон
`run_report.py --root pointerlock` (тесты `pointerlock-requires-gesture.html`,
`pointerlock_without_gesture.html`) + живая проба `--mcp-live-port`

## Симптом

```
pointerlock-requires-gesture.html:
  Request pointer lock without user gesture should fail
  - assert_unreached: Pointer lock should not be granted without a user gesture
    Reached unreachable code

pointerlock_without_gesture.html:
  pointerlock_without_gesture
  - assert_unreached: Must not acquire pointer lock. Reached unreachable code
```

Оба теста вызывают `element.requestPointerLock()` без предшествующего
пользовательского жеста (клик/клавиша) и ожидают, что спека (W3C Pointer Lock
L2 §3, "run the pointer lock steps" — error steps 2: no transient activation
→ reject + fire `pointerlockerror`) откажет в блокировке. Lumen выдаёт
блокировку безусловно.

Живая проба (`--mcp-live-port`, скрипт без единого пользовательского события)
подтверждает: сразу после `div.requestPointerLock()` `document.pointerLockElement`
указывает на этот `div` — блокировка выдана синхронно и безусловно.

## Причина

`request_pointer_lock` (`pointer_lock.rs:42-46`) — чистый сеттер состояния,
без единой проверки:

```rust
pub fn request_pointer_lock(element_nid: u32) {
    let mut s = state();
    s.locked_element_nid = Some(element_nid);
    s.pending_grab = Some(true);
}
```

Нативная привязка `_lumen_ptr_lock_request` (`v8_runtime.rs:3248-3250`) зовёт
эту функцию напрямую из JS-шима, тоже без проверки. Модуль сам документирует
себя как "Phase 0: in-memory lock" (`pointer_lock.rs:3`) — предусловия спеки
(transient activation, элемент connected, документ fully active, конфликт с
уже активной блокировкой на другом элементе) не реализованы вовсе, как и
событие `pointerlockerror` (`grep pointerlockerror crates/js/src/dom.rs` даёт
только объявления `onpointerlockerror: null`, ни одного `dispatchEvent`).

Тот же класс отсутствующей проверки user activation уже задокументирован для
двух других API того же движка: [BUG-365](BUG-365-FIXED.md) (`EyeDropper.open()`)
и [BUG-390](BUG-390-OPEN.md) (`requestFullscreen()`) — систематический пробел,
не специфичный для pointer lock.

## Как чинить

В `request_pointer_lock` (или в вызывающей JS-обвязке перед натив-вызовом)
добавить проверку transient activation по тому же паттерну, что предлагается
для BUG-390: при отсутствии жеста — не менять состояние, вернуть отказ, чтобы
JS-шим отклонил промис `requestPointerLock()` и продиспатчил `pointerlockerror`
на элементе (`Event('pointerlockerror', {bubbles: true})`, зеркально уже
существующему `pointerlockchange`). Понадобится общий источник "была ли
пользовательская активация с момента последнего клика/клавиши" — если такого
трекера ещё нет в движке, завести его сразу для всех трёх API (BUG-365/390/655)
одним хелпером, а не по одному на каждый.

Регрессия без WPT: `document.createElement('div').requestPointerLock()`,
вызванный из скрипта без предшествующего синтетического клика — промис должен
reject'иться, `document.pointerLockElement` — оставаться `null`.

## Связанные

Итоговый прогон категории (`--all --root pointerlock --recursive`, 20 id,
10/20 harness OK, 2/22 сабтестов) в основном упирается в отдельные, уже
заведённые дефекты: [BUG-622](BUG-622-OPEN.md) (`document.defaultView`
отсутствует — доминирующая причина `Error: Browsing context for element was
detached` в половине FAIL), [BUG-462](BUG-462-OPEN.md)/[BUG-574](BUG-574-OPEN.md)
(`Node.prototype.contains` — `elementDocument.contains is not a function`),
[BUG-596](BUG-596-OPEN.md) (`Event.prototype.initEvent` отсутствует —
переподтверждено живой пробой: `"initEvent" in new MouseEvent(...)` → `false`).
Часть id (`idlharness.window.html`, `mouse_buttons_back_forward.html`,
`movementX_Y_basic.html`, обе вариации `pointerlock-maintains-mousedown.html`,
`pointerlock_fullscreen-change.html`, `pointerlock_promise.html`,
`pointerlock_remove_target_on_mouseup.html`, `pointerlock_shadow.html`,
`pointerlock_unadjustedMovement.html`) TIMEOUT — используют
`test_driver.Actions().pointerMove(...)`/аналоги, которые
`executors/executorlumen.py::_handle_action` не реализует (только `click`),
известный инфраструктурный пробел, не движковый баг.
