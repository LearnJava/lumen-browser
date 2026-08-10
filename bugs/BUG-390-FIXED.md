# BUG-390 — `requestFullscreen()` не проверяет предусловия и никогда не отклоняет промис

**Статус:** FIXED 2026-08-10 (P3)
**Компонент:** js (`crates/js/src/dom.rs` — `requestFullscreen`/`exitFullscreen`
в `WEB_API_SHIM`)
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
всегда резолвился.

## Причина

`requestFullscreen` реализовывал только happy path:

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

Единственная проверка — `document.fullscreenEnabled`, но геттер захардкожен
(`get fullscreenEnabled() { return true; }`) — ветка `reject` мертва при любых
условиях. Ни одно из требуемых спекой предусловий не проверялось (отсоединённый
элемент, показанный popover, отсутствие transient activation), а событие
`fullscreenerror` не диспатчилось нигде: `grep fullscreenerror dom.rs` давал
только объявления `onfullscreenerror: null`.

## Что сделано (2026-08-10)

1. **`_lumen_fs_request_error(nid, el)`** — предусловия §4.3 в порядке спеки,
   возвращает `null` либо причину отказа: элемент не connected · чужой namespace
   (не HTML и не `svg`/`math`) · `document.fullscreenEnabled` ложен · элемент —
   показанный popover (`data-lumen-popover-open`) · нет transient activation
   (`navigator.userActivation.isActive`).
2. **`_lumen_fire_fullscreen_error(nid)`** — событие `fullscreenerror`
   (`bubbles: true`, `composed: true`, `cancelable: false`) на самом элементе
   через `_lumen_dispatch_rich`, поэтому `target` остаётся элементом, а
   document-слушатель видит его при всплытии; у отсоединённого элемента цепочки
   предков нет, поэтому событие идёт сразу на документ. `document.onfullscreenerror`
   — обычное свойство объекта `document`, а не запись в `_lumen_on_handlers`,
   поэтому хелпер зовёт его явно.
3. **`exitFullscreen()`** без fullscreen-элемента отклоняется `TypeError`
   (Fullscreen §4.4) вместо no-op-резолва — вторая половина `promises-reject.html`.
4. **`Event`** читает `composed` из init-словаря (DOM LS §2.2): раньше значение
   молча терялось, хотя один вызов в шиме его уже передавал.
5. **`onfullscreenchange`/`onfullscreenerror`** переехали из литерала обёртки
   элемента в `_LUMEN_EVENT_HANDLER_ATTRS`: как обычные `null`-свойства обёртки
   они были невидимы для диспатча (тот ищет обработчик в `_lumen_on_handlers` по
   nid), то есть `el.onfullscreenerror = fn` не срабатывал никогда.

**Проверка.** 11 новых юнит-тестов (`cargo test -p lumen-js --features v8-backend
v8_fullscreen_locks`, 43/43). WPT `fullscreen/api` тем же прогоном до и после:
harness 43/64 без изменений, сабтесты **88 → 89/163**, единственная разница —
`promises-reject.html` «Rejects if the element is not connected» FAIL → PASS,
регрессий нет; `fullscreen/model` 0/11 до и после.

## Остаток — [BUG-758](BUG-758-OPEN.md)

`document-onfullscreenerror.html` и `element-request-fullscreen-not-allowed.html`
остаются красными. Оба требуют отказа **по отсутствию transient activation**, а
`navigator.userActivation` в Lumen — замороженный литерал
`{ isActive: true, hasBeenActive: true }`: движок не отслеживает жесты вообще,
поэтому ветка активации в `_lumen_fs_request_error` при обычном ответе не
срабатывает (юнит-тест `request_fullscreen_rejects_without_transient_activation`
проверяет её, подменяя это единственное значение). Это не частный дефект
fullscreen: те же гейты в File System Access и Local Font Access читают то же
свойство — заведено отдельно как BUG-758, там же план и предупреждение про
синтетические клики в автоматизации.

## Связанные

* Тот же прогон отдельно подтвердил, что happy path (`body.requestFullscreen()`
  → `document.fullscreenElement !== null` → `exitFullscreen()`) работает.
* [BUG-391](BUG-391-OPEN.md) — вторая находка того же прогона (селекторы не
  бросают `SyntaxError`), не тронута.
