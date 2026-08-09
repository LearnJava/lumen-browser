# BUG-363 — `EventSource` не соответствует WebIDL: константы только на конструкторе, вызов без `new` не бросает, интерфейс не наследует `EventTarget`, атрибуты записываемые, невалидный URL не бросает `SyntaxError`

**Статус:** FIXED 2026-08-09
**Компонент:** js (`crates/js/src/dom.rs` — конструктор `EventSource` и его прототип)
**Найден:** P2, WPT-VENDOR-eventsource (2026-07-28), `run_report.py --all --root eventsource --recursive`

## Симптом

Шим определяет `EventSource` обычной JS-функцией с data-свойствами на инстансе.
Ни одно из требований WebIDL-биндинга к интерфейсу не выполнено. Проба
`--dump-layout` вне WPT (все строки — фактический вывод):

```
EventSource.CONNECTING                    = 0
EventSource.prototype.CONNECTING          = undefined
'CONNECTING' in EventSource.prototype     = false
instance.CONNECTING                       = undefined
call-without-new                          = NO THROW, got undefined
new EventSource("http://this is invalid/")= NO THROW, url=[http://this is invalid/]
new EventSource("") .readyState           = 2
typeof instance.dispatchEvent             = undefined
Object.getOwnPropertyDescriptor(EventSource.prototype,'onopen') = absent
hasOwnProperty(instance,'onmessage')      = true
hasOwnProperty(instance,'readyState')     = true
instance.url = "zzz"                      → YES-writable
```

Разбор по пунктам:

1. **Константы `CONNECTING`/`OPEN`/`CLOSED` определены только на объекте
   интерфейса.** По WebIDL константы объявляются и на interface object, и на
   interface prototype object, поэтому в браузере они видны и через инстанс
   (`source.CONNECTING`). В Lumen `EventSource.prototype.CONNECTING` ===
   `undefined`.
2. **Конструктор вызывается без `new`** и возвращает `undefined` вместо того,
   чтобы бросить `TypeError`.
3. **Интерфейс не наследует `EventTarget`**: `dispatchEvent` отсутствует;
   `addEventListener`/`removeEventListener` — самодельные методы на прототипе
   (`dom.rs:8233`, `dom.rs:8238`) поверх собственного реестра `this._listeners`,
   не связанного с общим механизмом событий.
4. **`url`/`readyState`/`withCredentials` — записываемые собственные
   data-свойства инстанса**, а по спеке это readonly-атрибуты (геттеры на
   прототипе). `source.url = "zzz"` молча меняет значение.
5. **`onopen`/`onmessage`/`onerror` — собственные свойства инстанса**, а не
   accessor-свойства на прототипе.
6. **Невалидный URL не бросает `SyntaxError` DOMException** — по спеке
   (HTML Living Standard §9.2.2, шаг 3) неудача парсинга URL обязана
   бросить `SyntaxError`.
7. **`readyState` синхронно становится `2` (CLOSED) прямо в конструкторе**, если
   соединение не удалось (`dom.rs:8218-8228`). По спеке конструктор всегда
   оставляет `readyState` равным `CONNECTING` (0), а отказ доставляется
   асинхронно событием `error`. Наблюдается на всех схемах, включая заведомо
   нефетчабельные (`ftp:`, `about:blank`, `mailto:`, `javascript:`) — везде
   `readyState=2` немедленно.

## Причина

`dom.rs:8193-8253` — интерфейс написан «вручную» как функция-конструктор:

```js
function EventSource(url, opts) {
    this.url = String(url || '');
    this.readyState = 0; // CONNECTING
    …
    var h = _lumen_sse_connect(this.url);
    if (!h) {
        this.readyState = 2; // CLOSED     ← п.7
        …
    }
}
EventSource.prototype.addEventListener = function(type, fn) { … };  ← п.3
EventSource.prototype.removeEventListener = function(type, fn) { … };
EventSource.prototype.close = function() { … };
EventSource.CONNECTING = 0;   ← п.1: только на конструкторе
EventSource.OPEN = 1;
EventSource.CLOSED = 2;
```

Проверки `new.target` нет (п.2), `Object.defineProperty` для readonly-атрибутов
и accessor-свойств не используется (п.4, п.5), наследование от `EventTarget` не
установлено (п.3), парсинг URL отсутствует вовсе (п.6 — см. BUG-362).

## Масштаб

В категории `eventsource` (61 id, 16/61 harness OK, 2/100 сабтестов) на эти
пункты приходятся почти все FAIL-сабтесты, дошедшие до выполнения (34 FAIL):

- п.1 — самый заметный: `eventsource-close.window.html` падает на первой же
  строке `assert_equals(source.readyState, source.CONNECTING, "connecting
  readyState")` → `expected (undefined) undefined but got (number) 2`. Одна
  строка теста ловит сразу п.1 (ожидаемое — `undefined`) и п.7 (фактическое —
  2 вместо 0). Тот же паттерн — во всех 4 сабтестах
  `eventsource-constructor-non-same-origin.window.html` и в 3 сабтестах
  `eventsource-cross-origin.window.html`;
- п.2 — `dedicated-worker/eventsource-constructor-no-new.any.html`, 0/1:
  `assert_throws_js: Calling EventSource constructor without 'new' must throw`;
- п.6 — `eventsource-constructor-url-bogus.any.html`, 0/1:
  `assert_throws_dom: function "() => { new EventSource("http://this is
  invalid/"); }" did not throw`;
- п.3 проявится после починки BUG-362: сейчас `eventsource-eventtarget.any.html`
  просто TIMEOUT, потому что соединение не открывается и слушатель не
  вызывается.

Отдельно: `eventsource-prototype.any.html` — один из двух PASS категории —
проверяет только расширяемость прототипа и `assert_own_property(self,
"EventSource")`, то есть проходит, несмотря на все семь пунктов выше.

За пределами WPT: п.1 (`source.CONNECTING`) и п.4 — распространённые идиомы в
клиентском коде SSE; п.3 значит, что `EventSource` не работает ни с одним
универсальным кодом, ожидающим `EventTarget`.

## Фикс

Весь блок (`_lumen_sse_fire`, `_lumen_sse_pump_one`, `_lumen_pump_sse`,
`EventSource` и его прототип) переписан по семи пунктам разом — один объект,
одни и те же строки, чинить по отдельности означало трогать их несколько раз:

- **п.1** — константы `CONNECTING`/`OPEN`/`CLOSED` теперь ставятся и на
  `EventSource`, и на `EventSource.prototype` через общий цикл
  `Object.defineProperty(...,{value,enumerable:true})` (writable/configurable
  по умолчанию `false` — совпадает с атрибутами WebIDL-константы).
- **п.2** — первая строка конструктора: `if (!new.target) throw new
  TypeError(...)`.
- **п.3** — `EventSource.prototype = Object.create(EventTarget.prototype)`,
  конструктор вызывает `EventTarget.call(this)`. Самодельные
  `addEventListener`/`removeEventListener` сняты — используются унаследованные
  из `EventTarget.prototype` (уже существовавший в шиме базовый класс,
  `dom.rs:392` — используется WebHID/WebUSB/Bluetooth/WebSerial/Navigation
  API и др., так что `_listeners` теперь в формате
  `{callback,capture,once}[]`, а не массив голых функций).
  `_lumen_sse_fire` больше не дублирует диспатч onopen/onmessage/onerror и
  ручной перебор `_listeners` — просто `ev.type = type; es.dispatchEvent(ev)`;
  `EventTarget.prototype.dispatchEvent` уже умеет и слушателей, и
  `this['on'+type]`.
- **п.4, п.5** — `url`/`readyState`/`withCredentials`/`onopen`/`onmessage`/
  `onerror` — accessor-свойства на `EventSource.prototype`
  (`Object.defineProperty`), читающие/пишущие приватные поля инстанса
  (`_url`/`_readyState`/`_withCredentials`/`_onopen`/`_onmessage`/`_onerror`).
  Все внутренние присваивания в конструкторе и в `_lumen_sse_pump_one`
  переведены на приватные поля напрямую (иначе присваивание через
  геттер-без-сеттера в нестрогом режиме молча не сработало бы).
- **п.6** — `new URL(_rawUrl, _lumen_loc_href)` теперь в `try`/`catch`,
  неудача даёт `throw new DOMException(..., 'SyntaxError')` вместо фолбэка на
  исходную строку. **Не закрывает WPT-кейс `eventsource-constructor-url-bogus`
  целиком** — самодельный `_lumen_parse_url` (BUG-693) принимает пробелы в
  хосте (`"http://this is invalid/"` даёт непустой `protocol`, поэтому `new
  URL(...)` для него не бросает), это лежит на стороне парсера URL, не этого
  бага.
- **п.7** — `readyState` не трогается в теле конструктора; при неудачном
  `_lumen_sse_connect` он остаётся `0` (CONNECTING), а `setTimeout(fn, 0)`
  (уже существовавший, теперь дополнительно выставляет `_readyState = 2`
  перед `error`) доставляет отказ асинхронно.

12 новых/переписанных unit-тестов в `dom::tests::v8_ws_sse`
(`cargo test -p lumen-js --features v8-backend eventsource`, 23/23 зелёных):
константы на интерфейсе и прототипе, `TypeError` без `new`, `SyntaxError`
DOMException на нерезолвящийся URL, `instanceof EventTarget` +
`dispatchEvent` работает с произвольным именем события, readonly-геттеры
(присваивание молча игнорируется, значение не меняется), `onmessage` —
accessor прототипа (не собственное свойство инстанса до и после присваивания).
Существовавший тест `eventsource_constructor_no_provider_sets_closed`
переименован и переписан на новое поведение (readyState синхронно `0`, только
после `_lumen_tick_timers()` — `2`). Полный прогон `cargo test -p lumen-js
--features v8-backend` (2544 теста) — без регрессий.
