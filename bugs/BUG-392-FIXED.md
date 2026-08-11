# BUG-392 — Gamepad API нарушает спеку: `getGamepads()` всегда 4 слота вместо 0, `onX`-обработчики на `Window` отсутствуют

**Статус:** FIXED 2026-08-11 (P3, ветка `p3-bug-392`)
**Компонент:** js (`crates/js/src/gamepad.rs:112` — `_gamepads` инициализация,
`:118-124` — `navigator.getGamepads`, `:147-151` — экспорт глобалов,
отсутствующие `ongamepadconnected`/`ongamepaddisconnected`)
**Найден:** P2, WPT-VENDOR-gamepad (2026-07-28), тест
`gamepad-supported-by-permissions-policy.html` + прямая проба
`--mcp-live-port`/`eval`

## Симптом

Прямая проба на пустой странице:

```js
navigator.getGamepads()        // → [null, null, null, null]  (длина 4)
'ongamepadconnected' in window     // → false
'ongamepaddisconnected' in window  // → false
```

По W3C Gamepad Level 2 §5.1, `navigator.getGamepads()` должен возвращать
массив длиной 0, пока в этой навигации не было подключено ни одного
геймпада (массив растёт лениво до наивысшего использованного индекса);
Chrome/Firefox без подключённого устройства отдают `[]`. Lumen всегда
возвращает 4 слота, все `null`, независимо от того, было ли подключение —
детектируемо любым кодом, который смотрит на `.length` вместо перебора
элементов на `!== null` (именно так делает вендоренный тест
`fs/not-fully-active.html`-класса `assert_equals(gamepads.length, 0, …)`,
и общий паттерн в реальных играх, определяющих "есть ли геймпады" через
`getGamepads().length > 0`).

`ongamepadconnected`/`ongamepaddisconnected` — стандартные event-handler
IDL-атрибуты на `Window` (наравне с `onclick`, `onload` и т.д.); спека
требует их наличия независимо от того, был ли когда-либо назначен
обработчик. WPT-паттерн feature-detection (`'onX' in window`) на Lumen
всегда даёт `false` для геймпадных событий, хотя `addEventListener`
для тех же типов событий работает (шим диспатчит `GamepadEvent` через
`window.dispatchEvent`, `_lumen_gamepad_connect`/`_lumen_gamepad_disconnect`,
`gamepad.rs:130-145`).

## Причина

`gamepad.rs:112`: `var _gamepads = [null, null, null, null];` — Phase 0
док-комментарий (`gamepad.rs:6-7`) сознательно фиксирует 4 слота как
временную заглушку "no hardware polling", но это же значение утекает
в наблюдаемую длину массива, возвращаемого `getGamepads()` — спека не
разрешает заранее объявленную ненулевую длину.

Шим никогда не определяет `window.ongamepadconnected`/
`window.ongamepaddisconnected` как accessor-свойства — только
инфраструктура `dispatchEvent`/`addEventListener`, унаследованная от
базового `Event`/`EventTarget`. В отличие от `onclick`/`onload` и
подобных, для которых где-то в шиме есть механизм регистрации
event-handler-IDL-атрибутов (не проверялось в рамках этого бага, но
раз `'onclick' in window` работает на реальных страницах — механизм
существует; здесь для `Window` он просто не подключён к типам `gamepad*`).

## Как чинить

1. `_gamepads` — заменить фиксированный 4-элементный массив на пустой
   (`[]`) и лениво расширять его до `index + 1` внутри
   `_lumen_gamepad_connect(index, …)` (аналогично тому, как реальные
   браузеры растят список только после первого события подключения);
   `getGamepads()` в Phase 0 (без реального железа) тогда всегда вернёт
   `[]`, что и корректно, и не требует отдельного связывания с шеллом.
2. Добавить `ongamepadconnected`/`ongamepaddisconnected` как
   accessor-свойства на `window`, использующие тот же паттерн
   event-handler-IDL, что и существующие `onX`-атрибуты (искать в шиме
   `dom.rs` механизм для `onclick`/`onload` и переиспользовать его для
   `gamepad*`-событий вместо ручного экспорта только классов).

Регрессия без WPT: на пустой странице `navigator.getGamepads().length === 0`
и `'ongamepadconnected' in window === true` без подключения устройства.

## Что сделано (2026-08-11)

1. `gamepad.rs`: `_gamepads` теперь `[]`; `_lumen_gamepad_connect` доращивает
   список до `index + 1` (`while (_gamepads.length <= i) _gamepads.push(null)`),
   `_lumen_gamepad_disconnect` гасит слот и снимает `connected`, но список НЕ
   укорачивает (спека §5.1: длина растёт до наивысшего использованного индекса
   и не сжимается). Без железа список остаётся пустым — `getGamepads()` → `[]`.
2. `gamepad.rs`: добавлены `window.ongamepadconnected`/`ongamepaddisconnected`
   как обычные nullable-свойства — та же форма, в которой основной шим объявляет
   `window.onpopstate`/`onhashchange`.
3. **Второй, не названный в заявке дефект — иначе п.2 был бы мёртвым кодом:**
   `window.dispatchEvent` (`dom.rs`, ветка `else`) вызывал только слушателей из
   `_other_win_listeners[type]` и НЕ смотрел на `window['on' + type]` —
   выделенные вызовы `on`-обработчика были только в ветках `load` и `error`.
   То есть присвоение `window.ongamepadconnected = fn` легло бы туда, куда
   диспатч не смотрит (класс BUG-390). Ветка `else` теперь общая: обработчик
   вызывается после явных слушателей, как в ветках `load`/`error` и в
   `_lumen_dispatch` для элементов. Двойного срабатывания нет — `load`/`error`
   обслужены ветками выше, а собственная доставка `hashchange`/`popstate`/
   `message` вызывает обработчик напрямую, минуя `dispatchEvent`. Побочно это
   чинит и `window.onscroll` (`_lumen_fire_window_scroll_event` шёл ровно в эту
   же ветку).

Тесты: `gamepad::tests::get_gamepads_empty_until_connect`,
`get_gamepads_grows_to_connected_index`,
`window_gamepad_event_handler_attributes_exist`, плюс два теста на полном
рантайме (`v8_runtime_with_dom`) —
`dom::tests::v8_event_classes::gamepad_surface_clean_without_device` (критерий
регрессии из этой заявки один-в-один) и
`window_on_handler_fires_for_generic_event_type` (порядок «слушатель →
`on`-обработчик» + рост списка до `index + 1`).

Заявка называла причиной только п.1 и п.2; п.3 — то, из-за чего п.2 сам по себе
ничего бы не дал.

## Связанные

* Категория `gamepad` — вне скоупа (🚫, аппаратный API), но найдена как
  побочный эффект вендоринга по постоянному решению пользователя (класс
  `accelerometer`/`fledge`/`eyedropper` — 🚫-scope не освобождает от
  спек-соответствия уже реализованной части API).
* Три из шести реально исполнившихся тестов категории (`not-fully-active.html`,
  `gamepad-permissions-policy-event-listener.html`) упираются не в это, а в
  уже задокументированный отдельный пробел — `<iframe>` без browsing context
  (`contentWindow`/`contentDocument === null`, класс BUG-381/BUG-383 из
  категории `focus`); `gamepad-supported-by-permissions-policy.html` падает
  на уже заведённый BUG-361 (`permissionsPolicy.features()` → `[]`);
  `idlharness-extensions.https.window.html` — сессионный артефакт класса
  BUG-380 (browsing context переиспользуется, результат отравлен предыдущим
  тестом); `idlharness.window.html` — невендоренный `/resources/idlharness.js`
  (не Lumen-баг, задокументированный разрыв survey-класса `FileAPI`).
