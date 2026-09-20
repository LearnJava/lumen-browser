# BUG-876 — распределение по слотам не происходит: `assignedNodes()` всегда пусто, `slotchange` не диспатчится нигде

**Статус:** FIXED 2026-09-20 (P6, ДОРАБОТКА → [GAP-SLOT](../ROADMAP.md))
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
[BUG-676](BUG-676-FIXED.md).

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

## Побочное исправление половины (BUG-878, 2026-09-13, `p1-gap-loadev-bug878`)

`assignedNodes()` возвращало пусто не из-за отсутствующей логики
распределения (фильтр по атрибуту `slot` уже был на месте,
`web_api_shim_mid.js`'s `assignedNodes`), а из-за того же нативного
дефекта, что BUG-878: `_lumen_get_shadow_root_host(slotNid)` не мог найти
хост, потому что искал его через `.parent` самого узла `ShadowRoot`
(никогда не установлен — `ShadowRoot` намеренно не DOM-ребёнок host'а),
вместо обратного поиска по карте `host -> root`. Фикс BUG-878
(`Document::shadow_host_of`) чинит и этот вызов —
`assigned_nodes_resolves_light_dom_slottable_via_shadow_host`
(`crates/js/src/dom/tests/v8_fontface_shadow_custom.rs`) подтверждает
`assignedNodes().length === 1` для ровно той разметки, что в §Симптом.

**Не тронуто (на тот момент):** диспатч `slotchange` — точки диспатча по-прежнему нет
нигде в кодовой базе, это отдельная, не начатая часть задачи. Статус
остаётся `OPEN`.

## Диспатч slotchange (2026-09-20, P6, `p6-gap-slot`)

Корень: `_lumen_fire_slotchange` (уже существовала, была подключена к
`appendChild`/`removeChild`, но молчала) искала `<slot>` через нескопированный
`_lumen_query_selector_all('slot')`. Эта функция обходит дерево от
`doc.root()` (`crates/engine/layout/src/selector_query.rs::query_all`), а
shadow root — orphan-узел арены, не DOM-ребёнок ничего достижимого от корня
документа (`Document::attach_shadow`'а doc-comment), поэтому обход НИКОГДА не
находил ни одного `<slot>` внутри теневого дерева — функция была тихим no-op
на любом реальном shadow-хосте, несмотря на то что её уже звали.

Фикс — заменить на `_lumen_query_selector_all_scoped(sr_nid, 'slot')` (тот же
нативный примитив, которым уже пользуются `Element`/`ShadowRoot`
`querySelector(All)`). Заодно:
* добавлен вызов `_lumen_fire_slotchange` во все точки мутации хоста, которых
  не было: `insertBefore`, `replaceChild`, `ChildNode.remove/before/after/replaceWith`,
  `ParentNode.prepend/append`;
* смена атрибута `slot` у light-DOM ребёнка тоже сигналит (хук в общей
  обёртке `_lumen_set_attr`, `web_api_shim_mid_b2.js`);
* `assignedSlot` (был хардкод-стаб `null`) теперь резолвит обратный поиск —
  слот в shadow-дереве родителя с совпадающим `name`.

Phase 0 упрощение: `_lumen_fire_slotchange` перебирает и шлёт `slotchange`
на все слоты shadow-дерева хоста, а не только на те, чей список назначенных
узлов реально изменился (DOM LS §4.2.2.4 требует по-слотово) — приемлемо для
движка без ручного назначения слотов (`slotAssignment: 'manual'` не
отслеживается).

Тесты (`crates/js/src/dom/tests/v8_fontface_shadow_custom.rs`):
`slot_slotchange_event_fires_on_append` (была плацебо-проверкой `changed >=
0`, независимо от того, произошёл ли реальный диспатч — переписана на
`changed === 1`), `slot_slotchange_event_fires_on_remove_and_onslotchange`,
`slot_slotchange_event_fires_on_insert_before`,
`slot_assigned_slot_resolves_matching_named_slot`. `cargo test -p lumen-js
--lib --features v8-backend` — 3970/3970; `cargo clippy -p lumen-js
--all-targets --features v8-backend -- -D warnings` чисто.

Статус — `FIXED`.
