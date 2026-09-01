# BUG-874 — `on<type>`-свойства уровня документа никогда не вызываются, а `in`-проверка на `window`/`document`/`navigation` отвечает `false`

**Статус:** OPEN (ДОРАБОТКА → [GAP-EVENTPATH](../ROADMAP.md))
**Тип:** нереализованная функциональность, не дефект реализованного кода — ведётся как задача `GAP-EVENTPATH` в [ROADMAP.md](../ROADMAP.md), P3 как баг не берёт. Переклассифицировано 2026-09-02 ре-триажем пула WPT-RUN-5/6: срезы заводили багом всё подряд, потому что правила заведения ([docs/probe-method.md §8](../docs/probe-method.md)) тогда ещё не было. Файл сохраняет номер и путь — на него ссылаются CLAUDE.md, STATUS-файлы и python-тулинг, а запись наблюдений остаётся полезной там, где лежит.
**Заведён:** 2026-08-23 (WPT-RUN-6, срез 27 — живой замер, варианты `handler-idl`/`cbx-report`/`navigation-onprops`)
**Область:** `crates/js/src/dom.rs:6249` — `document.dispatchEvent` обходит только `_lumen_listeners` документа и не заглядывает в `_lumen_on_handlers`; `crates/js/src/dom.rs:13795` — движковая доставка `readystatechange` идёт тем же `document.dispatchEvent`; таблица `_LUMEN_EVENT_HANDLER_ATTRS` (`:1010`) обслуживает только обёртки элементов
**Владелец:** P1/P3 (`lumen-js`). Заведён P2 в ходе WPT-задачи, здесь не чинится.

## Симптом

Присваивание `document.on<type> = fn` проходит (`typeof document.onresize`
после него — `"function"`), но обработчик не вызывается никогда: ни при
`document.dispatchEvent(new Event('resize'))`, ни при движковой доставке
(`document.onreadystatechange` молчит, хотя
`document.addEventListener('readystatechange', …)` на той же странице
срабатывает дважды — `interactive` и `complete`). На элементе тот же паттерн
работает: `meta.onresize = fn; meta.dispatchEvent(new Event('resize'))`
вызывает обработчик (BUG-360).

Вторая половина — детект свойства. `'onerror' in window`, `'onresize' in
document`, `'onnavigate' in navigation` — все `false`, хотя присваивание
«прилипает». WPT ловит именно этой идиомой (`'onX' in Y`) наличие поддержки,
так что тест либо уходит в ветку «не поддерживается», либо ждёт события,
которое некому доставить.

Третья, наблюдавшаяся здесь же: `<body onresize>` не форвардится в
`window.onresize` (HTML LS §8.1.7.3 «Window-reflecting body element event
handler set»); в BUG-360 это записано как известное отклонение — форвардится
только `onload`.

## Прямое измерение

`tests/wpt/verify_callback_import_preload_gaps.py --variant handler-idl`
(2026-08-23, dev-release, Linux, `main` = `34cbefd25`):

```
hidl-rsc-first=loading
hidl-rsc-set=function          ← присваивание прошло
hidl-body-set=function
hidl-doc-set=function
hidl-meta-set=function
hidl-meta-fired                ← элемент: работает
hidl-rsc-listener interactive  ← addEventListener на документе: работает
hidl-rsc-listener complete
hidl-load rs=complete
```

`hidl-rsc` (то есть `document.onreadystatechange`), `hidl-doc-fired` и
`hidl-body-fired` не напечатаны ни разу.

## Цена по WPT

* `html/webappapis/scripting/events/event-handler-onresize.html` — сабтест
  `document.onresize should set the document.onresize handler` (второй из
  трёх `async_test`) не завершается никогда.
* `html/dom/documents/resource-metadata-management/document-readyState.html` —
  сабтест `readystatechange event is fired each time document.readyState
  changes` держит файл: `t3` ждёт `document.onreadystatechange`.
* Шесть id `navigation-api/` ставят `navigation.onnavigate` /
  `navigation.oncurrententrychange` и без них не стартуют вовсе — там это
  накладывается на [BUG-881](BUG-881-OPEN.md) (событие и так не приходит).

## Что дальше

`document` в шиме — не обёртка элемента, а объектный литерал, поэтому мимо
него прошли обе половины BUG-360: и реестр `_lumen_on_handlers`, и
аксессоры `_lumen_define_on_handler_prop`. Минимальная починка — определить
на нём тот же curated-набор аксессоров и позвать `_lumen_get_on_handler` из
`document.dispatchEvent` (после явных слушателей, как в `_lumen_dispatch`).
`in`-проверка чинится тем же: аксессор, определённый через
`Object.defineProperty`, отвечает `true` на `in` без дополнительных мер.
