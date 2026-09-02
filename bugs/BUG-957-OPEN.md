# BUG-957 — `iframe.contentWindow` не является `EventTarget`: нет `addEventListener`/`removeEventListener`/`dispatchEvent`

**Статус:** OPEN
**Тип:** дефект реализованного кода — фасад окна фрейма (`winFacade`, `frame_bridge.rs`) собран как обычный `{}`-объект с ручными геттерами (`document`, `location`, `postMessage`, `close`, `name`) и ни разу не наделён методами `EventTarget`.
**Заведён:** 2026-09-02 (WPT-RUN-6, срез 34, живая проба `verify_slice34_gaps.py --variant frameset-synthetic-error-crosswindow`)
**Область:** js (`crates/js/src/frame_bridge.rs::winFacade`)
**Владелец:** P3.

## Симптом

Любой код, вызывающий `iframe.contentWindow.addEventListener(...)` (или
`.removeEventListener`/`.dispatchEvent`), падает с `TypeError:
… .addEventListener is not a function`. Живая проба это подтверждает
дословно: `window-addEventListener THREW TypeError: fw.addEventListener is
not a function`, где `fw = document.getElementById("f").contentWindow`.

`window.parent`/`window.top`/`window.frameElement` идут через тот же
`winFacade(bid)` (`installHierarchyAccessors` в том же файле), так что дефект
не ограничен `contentWindow` — он есть у любого объекта, который эта функция
когда-либо возвращает.

## Причина

```js
function winFacade(bid) {
    var cached = wins[bid];
    if (cached) return cached;
    var w = {};
    …
    Object.defineProperty(w, 'document', { … });
    Object.defineProperty(w, 'location', { … });
    w.close = function() {};
    w.postMessage = function(message, targetOrigin) { … };
    wins[bid] = w;
    return w;
}
```

`w` — голый `{}`, не связанный ни с каким `EventTarget`-прототипом и не
получающий `addEventListener`/`removeEventListener`/`dispatchEvent` вручную,
в отличие от `document`/`location`/`postMessage`, которые определены явно.
Ничего в файле их не устанавливает — это не частный случай пропущенной
проверки, метод отсутствует whole cloth.

## Прямое измерение

Живая проба (`--variant frameset-synthetic-error-crosswindow`, dev-release,
`main` = `76c58b60e`): `<iframe srcdoc="<frameset></frameset>">`,
`fw = iframe.contentWindow`. `fw.location.href`, `fw.document === iframe.
contentDocument`, `fw.document.querySelector(...)` — все работают.
`fw.addEventListener('error', …)` бросает `TypeError: fw.addEventListener is
not a function`. Тот же вызов на элементе документа фрейма
(`frameset.addEventListener('error', …)`) отрабатывает штатно — дефект
специфичен для фасада окна, не для событийной системы вообще.

## Кого это держит

Найден при разборе `html/webappapis/scripting/events/
event-handler-processing-algorithm-error/frameset-element-synthetic-
errorevent.html`/`-event.html`, которые строят `new EventWatcher(t,
framesetWindow, "error")` — конструктор `EventWatcher`
(`resources/testharness.js`) сам вызывает `watchedNode.
addEventListener(...)` без try/catch, так что это бросило бы тот же
`TypeError` внутри тела `promise_test`.

**Важно: это НЕ объясняет TIMEOUT именно этих двух id.** `promise_test`
оборачивает тело теста в `Test.prototype.step`, у которого есть свой
`try/catch`: пойманное исключение переводит тест в `FAIL` и сразу зовёт
`this.done()` (`testharness.js:2868-2880`) — то есть тест должен был бы
завершиться быстрым `FAIL`, а не зависнуть. Живая проба это подтверждает
косвенно: после брошенного исключения выполнение пробы продолжилось
дальше по скрипту (`frameset-addEventListener = ok`, `dispatch =
dispatched` напечатались) — то есть исключение не вешает страницу целиком.
Реальная причина TIMEOUT этих двух id осталась неустановленной в рамках
этого среза; оба id остаются в unclassified. Дефект заведён отдельно,
потому что он реальный, воспроизводимый и достаточно фундаментальный (любой
тест с `EventWatcher`/`addEventListener` на `window`/`top`/`parent`/
`frameElement` фрейма ловит то же самое), независимо от того, объясняет ли
он именно этот TIMEOUT.

## Направление починки

Наделить `winFacade`'s `w` реальным `EventTarget`-поведением — как минимум
`addEventListener`/`removeEventListener`/`dispatchEvent`, делегирующие в
существующую событийную машинерию фрейма (там, где она уже есть для
`postMessage`-доставки — `_lumen_f_post_message`/приёмный конец в дочернем
контексте), а не изобретать отдельную шину только для фасада.
