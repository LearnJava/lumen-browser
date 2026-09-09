# BUG-873 — `dispatchEvent` не распространяет событие по дереву: ни capture, ни bubble; настоящий клик доходит до `document`, но не до `window`

**Статус:** OPEN (ДОРАБОТКА → [GAP-EVENTPATH](../ROADMAP.md))
**Тип:** нереализованная функциональность, не дефект реализованного кода — ведётся как задача `GAP-EVENTPATH` в [ROADMAP.md](../ROADMAP.md), P3 как баг не берёт. Переклассифицировано 2026-09-02 ре-триажем пула WPT-RUN-5/6: срезы заводили багом всё подряд, потому что правила заведения ([docs/probe-method.md §8](../docs/probe-method.md)) тогда ещё не было. Файл сохраняет номер и путь — на него ссылаются CLAUDE.md, STATUS-файлы и python-тулинг, а запись наблюдений остаётся полезной там, где лежит.
**Заведён:** 2026-08-23 (WPT-RUN-6, срез 27 — живой замер, варианты `bubble-to-window`/`bubble-detail`/`body-error-bubble`)
**Область:** `crates/js/src/dom.rs:4696` — `dispatchEvent` обёртки элемента зовёт `_lumen_dispatch(nid, evt)` (`:1086`), то есть слушателей ОДНОГО узла, независимо от `evt.bubbles`; `crates/js/src/dom.rs:6249` — `document.dispatchEvent` обходит только реестр документа; `crates/js/src/dom.rs:1112` `_lumen_dispatch_bubble` и `_lumen_dispatch_rich` (пути настоящего ввода) поднимаются по предкам и заканчивают на `document`, `window` в цепочке нет вовсе
**Владелец:** P1/P3 (`lumen-js`). Заведён P2 в ходе WPT-задачи, здесь не чинится.

## Симптом

`el.dispatchEvent(new Event(type, {bubbles: true}))` вызывает только
слушателей самого `el`. Ни один предок — ни ближайший `<div>`, ни
`document.body`, ни `document`, ни `window` — не слышит событие, слушатель с
`capture: true` на предке тоже не вызывается. Событие, отправленное на
`document`, до `window` не доходит; событие, отправленное на `document.body`,
не доходит даже до `document`. `event.eventPhase` — `undefined`,
`composedPath()` отсутствует ([BUG-577](BUG-577-OPEN.md)).

Настоящий клик (шелл, `_lumen_dispatch_bubble`/`_lumen_dispatch_rich`) ведёт
себя иначе и наполовину правильно: он поднимается по предкам и доходит до
`document`, но `window` не обслуживает и там.

## Прямое измерение

`tests/wpt/verify_callback_import_preload_gaps.py --variant bubble-detail`
(2026-08-23, dev-release, Linux, `main` = `34cbefd25`). Слушатели `bd-evt`
стоят на `inner`, `outer` (в обеих фазах), `body`, `document` и `window`;
диспатч — один, с `bubbles: true`, на `inner`:

```
bd-at-inner target=true
bd-dispatch-returned=true
bd-click-at-outer          ← настоящий inner.click()
bd-click-at-document       ← он же
bd-clicked
```

`bd-at-outer`, `bd-at-outer-capture`, `bd-at-body`, `bd-at-document`,
`bd-at-window` не напечатаны ни разу; `bd-click-at-window` — тоже.
`--variant bubble-to-window` показывает ту же картину с трёх глубин
(`inner`/`body`/`document`) и добавляет две детали: `e.eventPhase` —
`undefined`, а у события, отправленного на `document`, `e.target === null`.

## Цена по WPT

`html/webappapis/scripting/events/event-handler-processing-algorithm-error/body-element-synthetic-event.html`
— `EventWatcher(t, window, "error")` ждёт на `window` событие, отправленное на
`document.body`; `body.onerror` при этом вызывается с одним аргументом, как
требует спека, то есть ломается ровно всплытие. Два соседних `frameset-*` id
той же папки упираются раньше в [BUG-480](BUG-480-OPEN.md) (нужен документ в
`<iframe>`). Механизм шире одного кластера: любой тест, слушающий на общем
предке событие, отправленное из скрипта (типовой паттерн делегирования),
получает молчание вместо события.

## Замер 2026-09-10 (P6, E2E-4 итерация 3): цена дефекта на живом React 18

Стенд [`samples/e2e4-hydration`](../samples/e2e4-hydration/README.md), настоящий
React 18.3.1 UMD, `hydrateRoot` в контейнер. React 18 вешает свои слушатели не
на элемент, а на **корень гидрации** и разводит их по фиберам, поэтому этот
дефект отрезает от React-приложения весь программный ввод целиком:

* **нативный клик — доходит.** MCP-`click` печатает `REACT synthetic onClick on
  #app`: `_lumen_dispatch_rich` поднимается до корневого контейнера, React
  строит синтетическое событие и зовёт обработчик. Событийная система React в
  Lumen работоспособна.
* **`dispatchEvent` — не доходит.** `btn.dispatchEvent(new MouseEvent('click',
  {bubbles: true}))` возвращает `true`, вызывает нативного слушателя самой
  кнопки (`native click on #btn`) — и на этом всё: до корня гидрации событие не
  поднимается, `onClick` React не вызывается, счётчик остаётся `clicked 0`.

Минимальный повтор без React (та же сборка, живое окно) — из `inner`
внутри `outer` внутри `body` слышит только `inner`, ни bubble, ни capture,
`e.eventPhase === undefined`.

Практическое следствие: любой сценарий автоматизации, синтезирующий ввод через
`dispatchEvent` (а это типовой способ для тест-раннеров и для кода самой
страницы), на React/Vue/Svelte-приложении молча не делает ничего — обработчик
живёт на корне, а событие туда не приходит.

## Что дальше

DOM Standard §2.9 «dispatching events» требует построить цепочку предков
(event path), пройти её в фазе capture, вызвать target и пройти обратно в
фазе bubble, с `window` последним звеном для узлов документа. Сейчас таких
проходов два независимых и оба неполные: скриптовый (`_lumen_dispatch`,
один узел) и нативный (`_lumen_dispatch_bubble`, предки + `document`).
Чинить имеет смысл одним общим обходом с фазами, иначе `eventPhase`,
`composedPath()` (BUG-577) и `window` придётся добавлять трижды.
