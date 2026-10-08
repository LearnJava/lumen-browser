# BUG-1458 — `test_driver.Actions().pointerMove(0, 0, {origin: el}).send()` под `wptrunner` не доставляет `mouseover`/`mousemove` и не включает `:hover`

**Статус:** OPEN
**Заведён:** 2026-10-08 (P2, WPT-RUN-14 срез 20, `css/selectors` + `css-pseudo` + `css-nesting` + `css-namespaces` + `css-cascade`)
**Область:** bidi-server/executor (`crates/bidi-server/src/protocol.rs:2311-2340` `replay_input_actions`; `tools/wptrunner/wptrunner/executors/executorlumen.py:600` `_action_action_sequence`)

## Симптом

На странице `<div id=h style="width:80px;height:40px">` с `#h:hover{color:green}` и слушателями `mouseover`, `mousemove`, `mouseenter`, `pointermove`, `pointerover` вызов `await new test_driver.Actions().pointerMove(0, 0, {origin: h}).send()` завершается успешно (промис разрешается), но после 500 мс ни одно событие не пришло, `h.matches(':hover')` = `false`, `getComputedStyle(h).color` = `rgb(0, 0, 0)`. Рядом работают `test_driver.click(el)` (событие `click` доставлено) и `test_driver.send_keys(el, 'a')` (`keydown` доставлен); `send_keys(document.body, '')` (Tab) переносит фокус на первый `tabindex=0`. `replay_input_actions` по комментарию поддерживает `pointerMove` (hover: `mouseover`/`:hover`, BUG-1194) — но `_action_action_sequence` переводит `origin: element` в абсолютную точку, а до страницы событие не доходит. Без `testdriver-actions.js` в странице `test_driver.Actions` — `TypeError: … is not a constructor` (это норма для страницы без подключения). Затронуты TIMEOUT: `selectors/hover-002.html`, `selectors/active-after-relayouts.html`, `selectors/active-toplayer-001.html`, `css-pseudo/marker-hit-testing.html` (20 сабтестов), `css-pseudo/events-on-pseudo-element*.tentative.html` (используют `Actions`); оставшиеся из 21 TIMEOUT среза с `click`/`send_keys` (`focus-visible-0NN`, `focus-visible-script-focus-0NN`, 17 id) — причина НЕ установлена: проба Tab на упрощённой странице проходит, а полный тест `focus-visible-script-focus-013.html` к концу таймаута оставляет `document.activeElement` = `<li>` и ловит `keydown`, но не `focusin` на документе.

## Проба

Страница пробы (`css/selectors/zz-…html`, вне репозитория, подключены `testdriver.js`, `testdriver-actions.js`, `testdriver-vendor.js`), запуск `run_corpus.py --prefixes …`:

| вызов | результат |
|---|---|
| `test_driver.click(button)` + слушатель `click` | PASS |
| `test_driver.send_keys(button, 'a')` + слушатель `keydown` | PASS |
| `test_driver.send_keys(document.body, '')` на странице с одним `tabindex=0` | PASS (`focus`, `focusin` на нём) |
| `new test_driver.Actions().pointerMove(0, 0, {origin: h}).send()` | промис разрешён, **событий нет, `:hover` ложь** |
| то же, 5 слушателей (`mouseover`, `mousemove`, `mouseenter`, `pointermove`, `pointerover`) | `[]` |

## Как найдено

WPT-RUN-14 срез 20: `selectors/hover-002.html` (2 сабтеста TIMEOUT), `active-after-relayouts.html`, `active-toplayer-001.html`, `css-pseudo/marker-hit-testing.html`.

## Что делать

Проверить, доходит ли `pointerMove` с элементом-origin до `replay_input_actions` и ставит ли он hover-состояние в окне `--no-paint`, в котором работает `wptrunner`; сверить с BUG-1194 (там hover подтверждён на окне).

## Как проверить

Проба из таблицы; `css/selectors/hover-002.html`.
