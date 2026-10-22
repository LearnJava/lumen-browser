# BUG-1458 — `test_driver.Actions().pointerMove(0, 0, {origin: el}).send()` под `wptrunner` не доставляет `mouseover`/`mousemove` и не включает `:hover`

**Статус:** FIXED 2026-10-22 (P3)
**Заведён:** 2026-10-08 (P2, WPT-RUN-14 срез 20, `css/selectors` + `css-pseudo` + `css-nesting` + `css-namespaces` + `css-cascade`)
**Область:** shell/js (`crates/shell/src/app/about_to_wait.rs` — `AutomationCommand::PointerMove`; `crates/js/src/v8_runtime/style_flush.rs`, `install/dom_core.rs` — зеркало `:hover` движкового потока)

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

## Исправление (2026-10-22, P3)

Дефектов было два, оба в окне, а не в BiDi-слое — `pointerMove` с `origin: element` доходил до `replay_input_actions` с верной точкой (проба через `run_smoke.py` с `clientX/clientY` в слушателях это показала).

1. **Точка без смещения контента.** `AutomationCommand::PointerMove {x, y}` по контракту несёт координаты контентного вьюпорта (как `Target::Point` у `Click`), но передавалась в `on_cursor_moved` как оконная: без `page_offset()` (высота тулбара `CHROME_H` = 68 px) курсор попадал на тулбар над целью, и страница не получала ни `mouseover`, ни смены `hovered_nid`. Клик это смещение добавлял в `resolve_automation_target`, поэтому `test_driver.click` работал. Теперь `PointerMove` прибавляет `page_offset()` так же.
2. **`:hover` не виден скрипту.** Даже при доставленном `mouseover` `matches(':hover')` был `false`, а `getComputedStyle(h).color` — цвет без `:hover`: движковый поток (`style_flush`, селекторные нативы) знал только зеркало фокуса (BUG-560), состояние `:hover` у него всегда было «нет». Добавлено зеркало `V8JsRuntime::hovered_nid` (+ `last_flushed_hover`, `IncrFlushBasis::hover`): оболочка при смене `hovered_nid` шлёт `notify_hover_changed` → `set_hovered_nid`; `maybe_flush` считает смену наведения причиной флаша без мутации DOM, `try_incremental_flush` добавляет корни `restyle_root_set_for_state_change` по наведению, `with_focus_state` ставит `:hover` на время `matches`/`querySelector*`.

Проверка: `css/selectors/hover-002.html` — TIMEOUT → OK (2/2), `hover-002.html.ini` удалён. Тесты: `get_computed_style_follows_the_hover_echo`, `get_computed_style_sees_hover_set_before_first_flush`, `matches_sees_the_hover_echo` (`crates/js/src/dom/tests/v8_bug1458_hover_mirror.rs`).

Не вошло: `active-after-relayouts.html`, `active-toplayer-001.html` (`:active` — зеркала нет, та же дыра, что была у hover), `css-pseudo/marker-hit-testing.html` и `events-on-pseudo-element*.tentative.html` по-прежнему TIMEOUT — другие причины, на `pointerMove` с origin они уже не упираются. Отдельный дефект, найденный пробой: `clientX/clientY` событий мыши/указателя в окне включают смещение контента (элемент в (8,8)–(88,48), курсор в его центре даёт `clientY = 96`, а не 28) — `js_*_event` получают оконные координаты, см. новый баг ниже.
