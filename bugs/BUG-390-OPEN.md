# BUG-390 — `requestFullscreen()` не проверяет предусловия и никогда не отклоняет промис

**Статус:** OPEN
**Компонент:** js (`crates/js/src/dom.rs:5650-5671` — `requestFullscreen`,
`crates/js/src/dom.rs:7171` — `document.fullscreenEnabled`)
**Найден:** P2, WPT-VENDOR-fullscreen (2026-07-28), прогон
`run_report.py --root fullscreen` (тесты `document-onfullscreenerror.html`,
`element-request-fullscreen-not-allowed.html`, `promises-reject.html`)

## Симптом

```
document-onfullscreenerror.html:
  Checks that the fullscreenerror event is fired when entering fullscreen fails
  - assert_unreached: Should have rejected: undefined Reached unreachable code

element-request-fullscreen-not-allowed.html:
  requestFullscreen() when not allowed to request fullscreen
  - assert_unreached: Should have rejected: undefined Reached unreachable code

promises-reject.html:
  Rejects if the element is not connected
  - assert_unreached: Should have rejected: Rejects if the element is not connected
    Reached unreachable code
```

Все три теста вызывают `element.requestFullscreen()` в ситуации, где спека
(WHATWG Fullscreen §4.3.2, "run the fullscreen steps") требует отклонения
промиса и (для первых двух) события `fullscreenerror`. Промис вместо этого
всегда резолвится.

## Причина

`requestFullscreen` (`dom.rs:5650-5671`) реализует только happy path:

```js
requestFullscreen: function(options) {
    var self = _obj;
    return new Promise(function(resolve, reject) {
        if (!document.fullscreenEnabled) {
            reject(new TypeError('Fullscreen not enabled'));
            return;
        }
        // ... enter fullscreen unconditionally ...
        resolve();
    });
},
```

Единственная проверка — `document.fullscreenEnabled`, но геттер
(`dom.rs:7171`) захардкожен: `get fullscreenEnabled() { return true; }` —
ветка `reject` мертва при любых условиях. Ни одно из требуемых спекой
предусловий не проверяется:

* элемент не присоединён к документу (`element is not connected`) —
  `promises-reject.html`;
* нет transient user activation (обычный случай без синтетического клика —
  `element-request-fullscreen-not-allowed.html`, `document-onfullscreenerror.html`);
* документ не fully active / element уже во fullscreen / popover conflict и т.д.
  (не покрыты этим прогоном, но тем же кодом).

Событие `fullscreenerror` нигде не диспатчится вовсе — `grep fullscreenerror
crates/js/src/dom.rs` даёт только объявления `onfullscreenerror: null`
(`dom.rs:5686`, `dom.rs:7188`) и `'onfullscreenerror' in ...` self-тесты, ни
одного `dispatchEvent(new Event('fullscreenerror'...))`.

## Как чинить

В `requestFullscreen` перед входом в fullscreen добавить последовательность
проверок из спеки (в порядке §4.3.2, error steps):
1. элемент не connected → reject(TypeError) + fire `fullscreenerror` на элементе;
2. элемент — popover, показанный auto/hint (конфликт) → reject + fire;
3. нет transient activation (или `options.navigationUI` не даёт override) →
   reject + fire;
4. namespace/fully active/document readiness — по месту.

Общий helper `fire_fullscreenerror(el)` должен диспатчить `Event('fullscreenerror',
{bubbles: true})` на элементе и на `document` (симметрично существующему
`fullscreenchange`-паттерну на строках 5661/5667-5668). `document.fullscreenEnabled`
при этом можно оставить `true` (Lumen не режет по permissions policy) — сам
геттер не источник бага, источник — отсутствующие проверки в `requestFullscreen`.

Регрессия без WPT: `document.createElement('div').requestFullscreen()`
(не connected) должен reject'иться TypeError; `document.body.requestFullscreen()`
без предшествующего пользовательского жеста в headless-скрипте — тоже.

## Связанные

* Тот же прогон отдельно подтвердил, что happy path (`body.requestFullscreen()`
  → `document.fullscreenElement !== null` → `exitFullscreen()`) работает —
  юнит-тесты `dom.rs:26908-26986` покрывают именно его.
