# BUG-810 — исполнитель WPT реализует только два testdriver-экшена из тридцати: остальные отклоняются, а отказ невидим странице — тест виснет вместо провала

**Статус:** OPEN (ДОРАБОТКА → [WPT-RUN-12](../ROADMAP.md))
**Тип:** нереализованная функциональность, не дефект реализованного кода — ведётся как задача `WPT-RUN-12` в [ROADMAP.md](../ROADMAP.md), P3 как баг не берёт. Переклассифицировано 2026-09-02 ре-триажем пула WPT-RUN-5/6: срезы заводили багом всё подряд, потому что правила заведения ([docs/probe-method.md §8](../docs/probe-method.md)) тогда ещё не было. Файл сохраняет номер и путь — на него ссылаются CLAUDE.md, STATUS-файлы и python-тулинг, а запись наблюдений остаётся полезной там, где лежит.
**Заведён:** 2026-08-21 (WPT-RUN-6, срез 17 — 193 id остатка снимка WPT-RUN-5, крупнейший механизм среза)
**Область:** `tools/wptrunner/wptrunner/executors/executorlumen.py:310-334` (`_handle_action` — ветки только для `click` и `generate_test_report`), связанное: `crates/bidi-server/src/protocol.rs::input_perform_actions` (BiDi-сторона, `pointer`+`key` уже реализована)
**Владелец:** P2 (обвязка WPT, дорожка WPT-RUN). Не движковый дефект.

## Симптом

Тест просит синтетический ввод и ждёт его завершения. Ожидание не
кончается ничем: ни события, ни ошибки, ни строки в логе — только TIMEOUT
враннера.

```js
// pointerevents/pointer-events-none-skip-scroll.html, сокращённо
const actions = new test_driver.Actions().scroll(50, 200, 0, 50, {duration: 50});
await actions.send();          // ← промис отклоняется, но страница этого не видит
```

## Прямое измерение

Исполнитель инструментирован на один прогон (`self.logger.info` в
`_handle_action`, снят после замера), `run_smoke.py` на двух тестах
`pointerevents`, dev-release, Linux, 2026-08-21:

```
0:09.69 INFO SLICE17-ACTION 'action_sequence'
0:19.68 TEST_END: Test TIMEOUT, expected OK. Subtests passed 0/1
```

То есть экшен действительно доходит до исполнителя, тот отвечает
`failure: action 'action_sequence' not implemented by Lumen's minimal WPT
executor` — и на этом всё: `testdriver-extra.js` отклоняет промис страницы, и
на момент этого замера отклонённый промис в Lumen не порождал ни
`unhandledrejection`, ни строки на stderr ([BUG-716](BUG-716-FIXED.md),
исправлен 2026-08-22), поэтому `testharness.js` провала не видел. TIMEOUT
вместо FAIL была обычная связка BUG-716/[BUG-591](BUG-591-FIXED.md), здесь с
обвязкой в роли источника — с фиксом BUG-716 этот конкретный случай должен
теперь доходить до `testharness.js` как FAIL с осмысленным сообщением, но
переизмерение прогоном за P2, не за этой заявкой.

Второй тест того же прогона
(`pointerevent_element_haspointercapture.html?mouse`) не дал даже этой
строки: его экшены адресованы элементом, а `get_context`
(`tools/wptrunner/wptrunner/testdriver-extra.js:112-120`) читает
`element.ownerDocument.defaultView` — которого нет
([BUG-622](BUG-622-OPEN.md), перепроверено пробой `--dump-layout`:
`typeof document.defaultView === "undefined"`) — и бросает «Browsing context
for element was detached» ещё на странице. Наблюдаемый исход тот же: тихий
TIMEOUT. Поэтому механизм в классификаторе один, а починки нужны две
независимые.

## Что реализовано и что нет

`_handle_action` знает ровно две ветки — `click`
(через `input.performActions`, уже поддержанный BiDi-сервером) и
`generate_test_report` ([BUG-659](BUG-659-FIXED.md)). Всё остальное из
`test_driver_internal` уходит в `ActionError`: `action_sequence`,
`send_keys`, `bless`, `set_permission`, `delete_all_cookies`,
`get_computed_role`/`get_computed_label`, `set_window_rect`,
`minimize_window`, `freeze`, `add_virtual_authenticator` и далее по списку
`testdriver-extra.js`.

Важно, что дешёвая часть уже есть: BiDi-сервер реализует
`input.performActions` с pointer- и key-источниками (SDC-2), а
`action_sequence` — это ровно тот же формат Actions, что WebDriver BiDi
принимает. То есть для самого крупного куска нужен не новый транспорт, а
трансляция payload'а экшена в вызов, который `_action_click` уже делает для
одной точки.

## Масштаб

Механизм `testdriver-action-unimplemented` в `tests/wpt/timeout_audit.py`
забирает **193 id** остатка снимка WPT-RUN-5 — крупнейший механизм среза 17
(для сравнения: вся `layout-instability` — 35). Распределение по
категориям:

| категория | id |
|---|---|
| `pointerevents` (включая `pointerlock`) | ~85 |
| `editing` + `html/editing` (dnd, whitespaces, run) | ~30 |
| `uievents` (mouse, focus-events, keyboard) | ~20 |
| `css/selectors` (`focus-visible-*`) | 11 |
| `pointerlock`, `touch-events`, `selection`, `inert`, `input-events` | ~20 |
| прочее (`css/css-pseudo`, `css/css-view-transitions`, `html/semantics`, `html/canvas`, `accname`, `fetch/metadata`) | ~27 |

Это нижняя оценка: id, где отказ экшена печатает исключение, уже разобраны
стадией улик (`defaultview-test-driver` — 295 id только в `editing`), и в
эти 193 не входят.

## Направление починки (не предписание)

1. `action_sequence` → `session.input.perform_actions` — самый большой
   выигрыш на единицу работы; формат почти совпадает, транспорт есть.
2. `send_keys` → тот же `input.performActions` с key-источником.
3. `bless`, `delete_all_cookies`, `set_permission` — дешёвые одиночные
   вызовы, каждый закрывает свой хвост категорий.
4. Всё, что останется нереализованным, стоит хотя бы **логировать**
   (`self.logger.info` на ветке `ActionError`): сейчас отказ не виден
   вообще нигде, и именно поэтому механизм пришлось искать маркером по
   исходникам, а не читать из логов прогона.

Ортогонально и не здесь: `document.defaultView` ([BUG-622](BUG-622-OPEN.md))
блокирует любые экшены с элементом-мишенью ещё до транспорта, а BUG-716
превращает каждый отказ в зависание вместо провала.

## Как проверить фикс

1. `run_smoke.py --binary target/dev-release/lumen
   /pointerevents/pointer-events-none-skip-scroll.html` — TEST_END не
   TIMEOUT.
2. `run_report.py --all --root pointerevents --recursive` — доля harness OK
   растёт (снимок WPT-RUN-5: 93 из 132 TIMEOUT категории неразобраны).
3. `timeout_audit.py --json` на свежем прогоне: механизм
   `testdriver-action-unimplemented` уменьшается, а не переезжает в
   `unclassified`.
