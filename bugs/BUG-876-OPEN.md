# BUG-876 — распределение по слотам не происходит: `assignedNodes()` всегда пусто, `slotchange` не диспатчится нигде

**Статус:** OPEN (ДОРАБОТКА → [GAP-SLOT](../ROADMAP.md))
**Тип:** нереализованная функциональность, не дефект реализованного кода — ведётся как задача `GAP-SLOT` в [ROADMAP.md](../ROADMAP.md), P3 как баг не берёт. Переклассифицировано 2026-09-02 ре-триажем пула WPT-RUN-5/6: срезы заводили багом всё подряд, потому что правила заведения ([docs/probe-method.md §8](../docs/probe-method.md)) тогда ещё не было. Файл сохраняет номер и путь — на него ссылаются CLAUDE.md, STATUS-файлы и python-тулинг, а запись наблюдений остаётся полезной там, где лежит.
**Заведён:** 2026-08-23 (WPT-RUN-6, срез 27 — живой замер, варианты `slotchange`/`slot-detail`/`slot-detail2`)
**Область:** `crates/js/src/dom.rs:5086` — `assignedNodes` возвращает результат обхода, который на живом дереве даёт пусто; `grep -rn "'slotchange'" crates/` — ни одной точки диспатча (имя есть только в списке `_LUMEN_EVENT_HANDLER_ATTRS`, `:1054`)
**Владелец:** P1/P3 (`lumen-js`). Заведён P2 в ходе WPT-задачи, здесь не чинится.

## Симптом

Хост с обычной light-DOM-разметкой `<div id=host><div slot="s1">…</div></div>`
и `<slot name="s1">` внутри его shadow root: `slot.assignedNodes()` возвращает
пустой массив — и сразу после `attachShadow`, и после того как в хост
добавлен ещё один ребёнок с `slot="s1"`. Событие `slotchange` не приходит ни
слушателю `addEventListener`, ни свойству `onslotchange`: в воркспейсе нет ни
одной точки, откуда оно диспатчилось бы.

Соседние части Shadow DOM при этом исправны: `attachShadow` возвращает
объект, `root.innerHTML` пишется и читается, `root.querySelector('slot')`
находит слот, `document.createElement('slot')` даёт `HTMLSlotElement` с
методами `assignedNodes`/`assignedElements`.

## Прямое измерение

`tests/wpt/verify_callback_import_preload_gaps.py --variant slot-detail2`
(2026-08-23, dev-release, Linux, `main` = `34cbefd25`):

```
sd2-innerHTML-set
sd2-innerHTML-read "<slot name=\"s1\"></slot>"
sd2-query found
sd2-appended ctor=HTMLSlotElement assignedNodes=function
sd2-assigned n=0        ← light-DOM ребёнок с slot="s1" уже в хосте
sd2-host-appended n=0   ← добавили второй такой же
sd2-checked
```

`sd2-slotchange` не напечатан ни разу. Побочно замерено: `root.childNodes`
у shadow root отсутствует (`no-childNodes`) — литеральная природа объекта,
[BUG-676](BUG-676-OPEN.md).

## Цена по WPT

* `shadow-dom/slotchange.html` — сабтест `slotchange event: Append a child to
  a host (onslotchange).`;
* `shadow-dom/inserting-fragment-under-shadow-host.html` — сабтест про
  вставку `DocumentFragment`.

Оба ждут события в `async_test`, поэтому это TIMEOUT, а не FAIL. Категория
`shadow-dom` вендорена и прогнана (BUG-676: 198/276 harness OK), так что
цена ограничена тестами, чья проверка идёт именно через распределение.

## Что дальше

DOM Standard §4.2.2.4 «assign slottables»: при вставке/удалении ребёнка
хоста и при изменении атрибута `slot` нужно пересчитать назначение и
поставить `slotchange` в очередь микрозадач для затронутых слотов. Сейчас
пересчёта нет вовсе — `assignedNodes` считает по дереву в момент вызова и
на живом хосте даёт пусто, а очереди `slotchange` не существует.
