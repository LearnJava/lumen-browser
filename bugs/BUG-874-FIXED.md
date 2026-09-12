# BUG-874 — `on<type>`-свойства уровня документа никогда не вызываются, а `in`-проверка на `window`/`document`/`navigation` отвечает `false`

**Статус:** FIXED 2026-09-12 (ДОРАБОТКА → [GAP-EVENTPATH](../ROADMAP.md), задача закрыта)
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

## Замер 2026-09-10 (P6, попутно с [BUG-873](BUG-873-FIXED.md)): первая треть снята

Общий обход события §2.9 сделал `document` записью пути наравне с элементом,
и его шаг вызывает `document['on' + type]` после явных слушателей. Живая
проба (`--dump-layout`, `dev-release`) на минимальной странице:

```
doc.onresize fired                        ← было: молчание
'onresize' in document = false            ← до присваивания, без изменений
'onerror' in window   = false             ← без изменений
'onreadystatechange' in document = false  ← без изменений
```

То есть закрыт только пункт «присваивание проходит, обработчик не
вызывается никогда» для `document.dispatchEvent`. **Осталось открытым:**

* `in`-детект — аксессоры curated-набора на `document`/`window`/`navigation`
  так и не объявлены, а именно этой идиомой WPT определяет поддержку;
* движковая доставка `readystatechange`: она идёт своим путём, а не через
  `document.dispatchEvent`, так что `document.onreadystatechange` всё ещё
  молчит там, где `addEventListener` срабатывает дважды;
* форвард `<body onresize>` → `window.onresize` (HTML LS §8.1.7.3).

Раздел «что дальше» выше остаётся в силе целиком — вторая его фраза (позвать
`_lumen_get_on_handler` из `document.dispatchEvent`) уже не нужна.

## Починено 2026-09-12 (P1, GAP-EVENTPATH): `in`-детект добавлен, readystatechange оказался уже рабочим

Заявленных «осталось открытым» пункта было три, реально сломан — один.
Живая проба (`cargo test -p lumen-js --features v8-backend`, временный тест
в `crates/js/src/dom/tests/v8_event_propagation.rs`) перед правкой:

```
rsc-fired:interactive                 ← сработал! `_lumen_apply_ready_state`
                                         зовёт `document.dispatchEvent(rsEv)`,
                                         а тот с 2026-09-10 (BUG-873) уже читает
                                         document['on'+type] на фазе target —
                                         пункт «движковая доставка мимо
                                         document.dispatchEvent» устарел, замер
                                         2026-09-10 выше его не перепроверил
onresize-in-document:false            ← реально сломано
onerror-in-window:false               ← реально сломано
```

Правка — только «что дальше» пункт про `in`: `document`/`window` строятся как
объектные литералы (не проходят через `_lumen_define_on_handler_prop`,
который рассчитан на элементы с `__nid__`), поэтому им никогда не хватало
самого объявления свойства. Раз оба объекта уже читают `on<type>` простым
скобочным доступом (`document['on'+event.type]` в `_lumen_invoke_at`,
`window['on'+evt.type]` в generic-ветке `window.dispatchEvent`), обычного
`obj[attrName] = null` на каждое имя из уже существующего curated-списка
`_LUMEN_EVENT_HANDLER_ATTRS` достаточно — доставка не тронута, только
объявление. `document.onreadystatechange`/`document.onvisibilitychange`
добавлены отдельно (Document-only, не входят в GlobalEventHandlers).
`hasOwnProperty`-охрана в обоих циклах (`web_api_shim_mid.js` для `document`,
`web_api_shim_mid_b.js` для `window`) не даёт затереть уже объявленные
свойства с особой семантикой (`onfullscreenchange`, `onload`, `onscroll`, …).

Форвард `<body onresize>` → `window.onresize` (HTML LS §8.1.7.3) в скоуп не
взят: это отдельный, более рискованный кусок (риск двойной доставки
resize/scroll/focus/blur через путь `window`, см. комментарий у
`_LUMEN_BODY_FORWARDED_TO_WINDOW` в `web_api_shim_mid.js`), не входит в
измеренную цену WPT этого бага (оба id — `event-handler-onresize.html`,
`document-readyState.html` — про `document`/`window`, не про `<body>`), и уже
был помечен как известное узкое отклонение до этого бага. `navigation`-часть
(`'onnavigate' in navigation` и три соседних) не тронута — сам объект
`navigation` в шиме отсутствует полностью, это форма [BUG-881](BUG-881-OPEN.md),
а не этого бага.

`cargo test -p lumen-js --features v8-backend`: 3585/3585 зелёных (3 новых
теста в `v8_event_propagation.rs`), `cargo clippy -p lumen-js --all-targets
--features v8-backend -- -D warnings` чист. `scripts/scoped-test.sh`: один
красный (`lumen-driver::cases::snapshot_cpu::cpu_snapshots_match_references`,
7 картинок) — подтверждено A/B на `main` тем же прогоном, тот же байт-в-байт
список несовпадений — предсуществующий дрейф, не регрессия этой правки.
