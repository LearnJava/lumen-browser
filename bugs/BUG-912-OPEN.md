# BUG-912 — у событий нет class string: `Object.prototype.toString.call(new Event('x'))` → `[object Object]`

**Статус:** OPEN
**Заведён:** 2026-08-25 (P1, при закрытии [BUG-838](BUG-838-FIXED.md) — измерен WPT-прогоном, не чтением кода)
**Область:** `crates/js/src/dom.rs` — 26 конструкторов событий шима; `Symbol.toStringTag` есть ровно у одного (`ErrorEvent`, `dom.rs:737`, заведён попутно с BUG-591/813). Плюс копии в `crates/js/src/worker.rs:389` (`_LumenWorkerErrorEvent` — есть) и в остальных пофичевых шимах (нет).
**Владелец:** P3 (`lumen-js`)

## Симптом

```js
Object.prototype.toString.call(new Event('x'))        // "[object Object]", ожидается "[object Event]"
Object.prototype.toString.call(new CustomEvent('x'))  // "[object Object]", ожидается "[object CustomEvent]"
```

WebIDL §3.7.3 требует, чтобы у прототипа каждого интерфейса было
неперечислимое, незаписываемое, настраиваемое `@@toStringTag` со значением —
именем интерфейса. `testharness.js` проверяет это отдельным ассертом
(`assert_class_string`), и он стоит в WPT в конце длинной цепочки проверок
события, то есть срабатывает уже после того, как всё содержательное сошлось.

## Прямое измерение

`run_report.py --all --root html/semantics/scripting-1/the-script-element/fetch-src --recursive`
(2026-08-25, dev-release, Windows, после починки BUG-838):

```
FAIL Script src with an empty URL   - assert_class_string: expected "[object Event]" but got "[object Object]"
FAIL Script src with an invalid URL - assert_class_string: expected "[object Event]" but got "[object Object]"
```

Три id (`fetch-src/empty.html`, `empty-with-base.html`, `failure.html`)
проходят все предыдущие ассерты (`type`, `bubbles`, `cancelable`, `isTrusted`,
`target`, `instanceof Event`) и падают только здесь. Категория — лишь то место,
где дефект попался: он общий для всей иерархии событий, а `failure.html`
(невалидный URL внешнего скрипта) к пустому `src` отношения не имеет вовсе.

## Причина

`Event`/`CustomEvent` и ещё 24 подкласса объявлены в шиме обычными
ES5-конструкторами (`function Event(type, init) { … }`) — у их прототипов нет
`@@toStringTag`, поэтому `Object.prototype.toString` падает на ветку
«обычный объект».

## Направление починки (не предписание)

Проставить `@@toStringTag` **каждому** классу отдельно, а не одному
`Event.prototype`: тег наследуется по цепочке прототипов, так что один тег на
базе заставит `MouseEvent` отвечать `[object Event]` — не менее неверно, чем
`[object Object]`, и заметно более обманчиво. Форма — та же, что уже применена
к `ErrorEvent`:

```js
Object.defineProperty(Event.prototype, Symbol.toStringTag, {
    value: 'Event', writable: false, enumerable: false, configurable: true
});
```

Перед правкой стоит пересчитать список: 26 — это только `dom.rs`; события
объявляются и в пофичевых шимах (`web_audio.rs`, `video_bindings.rs`,
`speech.rs`, …), каждый из которых — собственный `rt.eval` и правку в
`WEB_API_SHIM` не наследует (урок BUG-780).

Смежный дефект того же класса, но на другом объекте — [BUG-589](BUG-589-OPEN.md)
(`window` не отвечает `[object Window]`).

## Как проверить фикс

1. `run_report.py --all --root html/semantics/scripting-1/the-script-element/fetch-src --recursive`
   — три указанных id должны стать PASS целиком.
2. Прогнать категорию `dom/events` до и после: ассерт стоит во многих тестах
   событий, так что цифра по ней и есть мера.
