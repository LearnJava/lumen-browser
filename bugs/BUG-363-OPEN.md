# BUG-363 — `EventSource` не соответствует WebIDL: константы только на конструкторе, вызов без `new` не бросает, интерфейс не наследует `EventTarget`, атрибуты записываемые, невалидный URL не бросает `SyntaxError`

**Статус:** OPEN
**Компонент:** js (`crates/js/src/dom.rs:8193-8253` — конструктор `EventSource` и его прототип)
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

## Возможный фикс (не реализован в этой сессии)

- п.1: продублировать три константы на `EventSource.prototype` (не забыть, что
  по WebIDL они неперечисляемые, неизменяемые и неконфигурируемые).
- п.2: `if (!new.target) throw new TypeError("Failed to construct 'EventSource':
  Please use the 'new' operator")` первой строкой конструктора.
- п.3: назначить прототип от `EventTarget.prototype` и снять самодельные
  `addEventListener`/`removeEventListener`, переведя `_lumen_sse_fire` на общий
  диспатч. Это самый крупный пункт; связан с BUG-360 (живые пути диспатча
  читают только реестр `addEventListener`) — чинить согласованно.
- п.4, п.5: перевести `url`/`readyState`/`withCredentials` на
  `Object.defineProperty` с геттерами поверх приватных полей, а
  `onopen`/`onmessage`/`onerror` — на accessor-свойства прототипа.
- п.6: бросать `SyntaxError` DOMException при неудаче парсинга — естественно
  делается тем же изменением, что BUG-362 (резолв через `new URL(...)` в
  `try`/`catch`).
- п.7: не трогать `readyState` в конструкторе; оставлять `0` и доставлять отказ
  асинхронным `error`-событием, как уже делает существующий `setTimeout(…, 0)`
  рядом (`dom.rs:8220`).

Пункты 1, 2, 6, 7 — небольшие локальные правки; пункт 3 — отдельная работа.
Заведено одним багом, потому что это один объект и один блок кода, и чинить их
по отдельности означает трогать те же строки несколько раз.

Не чинится в этой сессии — P2-wpt вендорит и обследует, фиксы кода — дорожка P3
(`CLAUDE.md`, назначения разработчиков).
