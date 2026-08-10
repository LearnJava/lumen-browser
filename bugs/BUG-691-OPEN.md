# BUG-691 — `TextEvent` global constructor and `UIEvent.prototype.pseudoTarget` missing entirely

**Статус:** OPEN
**Компонент:** js (`crates/js/src/dom.rs`, `WEB_API_SHIM` — event-class block at `dom.rs:432` onward: `UIEvent`/`MouseEvent`/`KeyboardEvent`/`InputEvent`/`FocusEvent`/`WheelEvent`/`PointerEvent`/`AnimationEvent`/… has no `TextEvent` sibling and no `pseudoTarget` member on `UIEvent.prototype`)
**Найден:** P2, WPT-VENDOR-uievents, 2026-08-09

## Симптом

Категория `uievents` (`tests/wpt/uievents/`, 146 файлов, пин `35be3b44`) —
вендорена и прогнана целиком (`run_report.py --all --root uievents
--recursive`, ~6:41, 70 отобранных id): **33/70 harness OK, 17/127 сабтестов**.
Два независимых, ранее нигде не заведённых дефекта, оба в `uievents/textInput/
api.html` и `uievents/ui_event_pseudo_target.html`:

1. `window.TextEvent` (легаси-интерфейс DOM3 Events, всё ещё в UI Events §4.4)
   не существует вовсе — `new TextEvent('textInput')` бросает
   `ReferenceError: TextEvent is not defined`, а не `TypeError` (тест ожидает,
   что интерфейс существует, но напрямую не конструируется — `assert_throws_js
   (TypeError, …)`). Отдельно от `document.createEvent` ([BUG-590](BUG-590-OPEN.md),
   тоже отсутствует целиком): даже если бы `createEvent('TextEvent')` работал,
   `Object.getPrototypeOf(e) === window.TextEvent.prototype` всё равно упал бы
   на отсутствующем глобале. 1 файл, 6 сабтестов (`No constructor`,
   `document.createEvent('TextEvent') prototype chain`, `initTextEvent()` ×3,
   case-sensitivity textInput/textinput) — 2 из них (No constructor +
   case-sensitivity, единственные не зависящие от `createEvent`) дали текст
   `ReferenceError: TextEvent is not defined` в прогоне; остальные замаскированы
   BUG-590 первым же вызовом `document.createEvent`.

2. `UIEvent.prototype.pseudoTarget` (UI Events §idl-uievent, legacy
   `EventTarget`-alias для событий, синтетически ретаргетированных на границе
   shadow DOM/detached-элементов) не определён — `'pseudoTarget' in
   UIEvent.prototype === false`, никакого геттера на прототипе ни у `UIEvent`,
   ни у наследующего `MouseEvent`. `ui_event_pseudo_target.html`: 0/2 сабтеста.

## Масштаб

Узкое, оба дефекта — по одному файлу каждый, 8 сабтестов суммарно из 127 в
категории. Не пересекается с доминирующим сигналом прогона (110 из 127
FAIL/TIMEOUT сабтестов — переподтверждение уже открытых
[BUG-574](BUG-574-OPEN.md) (`Node.contains` отсутствует, ломает
`test_driver.click()`/`send_keys()` — 34 сабтеста), [BUG-622](BUG-622-OPEN.md)
(`document.defaultView` отсутствует → «Browsing context for element was
detached» на всех тестах с `<iframe>` — 52 сабтеста), [BUG-590](BUG-590-OPEN.md)
(`document.createEvent` отсутствует целиком — 18 сабтестов) и
[BUG-384](BUG-384-FIXED.md) (именованный доступ `window.<id>` не реализован,
`ReferenceError: square is not defined` в `order-of-events/mouse-events/
click-on-div.html` — 2 сабтеста)). Оба новых дефекта того же класса, что
[BUG-680](BUG-680-OPEN.md)/[BUG-688](BUG-688-OPEN.md)/[BUG-687](BUG-687-OPEN.md):
WebIDL-поверхность интерфейса, объявленного спекой, не установлена на
глобале/прототипе вовсе, при том что близкие соседние интерфейсы (`UIEvent`
сам, `MouseEvent`, `KeyboardEvent`) реализованы полноценно рядом в том же
файле.

## Причина

`WEB_API_SHIM` (`crates/js/src/dom.rs:432-…`) объявляет цепочку event-классов
(`UIEvent` → `MouseEvent`/`KeyboardEvent`/`InputEvent`/`FocusEvent` →
`WheelEvent`/`PointerEvent`, отдельно `AnimationEvent`/`TransitionEvent`/…), но
не содержит ни `function TextEvent(...)`, ни определения `pseudoTarget` на
`UIEvent.prototype` — оба члена никогда не были добавлены, не регрессия.

## Дальше

Fix scope: (1) добавить `TextEvent` рядом с `KeyboardEvent`/`InputEvent` в том
же блоке — наследует `UIEvent`, `initTextEvent(type, bubbles, cancelable,
view, data)` legacy-инициализатор (аналог `initEvent`/`initUIEvent` уже
предположительно существующих для обратной совместимости — проверить), `data`
как единственное специфичное поле, дефолт `'undefined'` (строка, по тесту
`initTextEvent('foo')` без `data` даёт `e.data === 'undefined'`, не `undefined`
как значение — особенность легаси-спеки, сверить с тестом дословно); завести
конструктор так же, как `UIEvent`/`MouseEvent` (не throw при `new
TextEvent(type)`, `assert_throws_js(TypeError, …)` в тесте относится только к
вызову вовсе без аргумента `type`, не к самому конструктору). (2) добавить
геттер `pseudoTarget` на `UIEvent.prototype` (`Object.defineProperty` с `get`,
возвращающим `null` при отсутствии ретаргетинга — простейшая
спецификационно-допустимая реализация, раз shadow DOM retargeting сам по себе
не влияет на остальной сигнал прогона). Оба фикса не требуют
`document.createEvent`/BUG-590 — независимо верифицируемы через
`--mcp-live-port` или повторный `run_report.py --root uievents`.
