# BUG-1055 — `appendChild` молча роняет детач-узлы (`new Comment()`/`new Text()`/`createProcessingInstruction`), вставленные в живое дерево

**Статус:** OPEN
**Заведён:** 2026-09-15 (GAP-XMLDOC срез 25 P1, живой прогон `tests/wpt/run_report.py --all --root dom/nodes`, `dom/nodes/rootNode.html`)
**Область:** js (`crates/js/src/shim/web_api_shim_mid.js` — арена-элементный `appendChild`, `~6718`)
**Владелец:** P3

## Симптом

```js
var parent = document.createElement('div');
var pi = document.createProcessingInstruction('target', 'data');
parent.appendChild(pi);
pi.getRootNode();   // спека: parent; факт: pi (вставка не произошла)
```

Причина — в самом арена-backed `appendChild` (используется каждым живым
элементом, `_lumen_make_element`-обвязка):

```js
appendChild: function(c) { var nid = this.__nid__;
    ...
    if (!c || c.__nid__ === undefined) return c;   // <-- здесь
```

`c.__nid__ === undefined` истинно для ЛЮБОГО детач-узла — `new Comment()`,
`new Text()`, `document.createProcessingInstruction(...)` — все три не
арена-backed по конструкции (см. `_lumen_make_character_data`/
`_lumen_make_processing_instruction`, `crates/js/src/shim/web_api_shim_mid.js`).
Ветка трактует это как «нечего вставлять» и тихо возвращает `c` без вставки,
без исключения — не `TypeError`, не `HierarchyRequestError`, просто no-op.
`rootNode.html`'s «с одним предком» и «внутри документа» сабтесты для PI это
и ловят (третий/четвёртый тест файла): `getRootNode()` после `appendChild`
продолжает возвращать сам узел, будто вставки не было.

## Цена

Обнаружено на PI (GAP-XMLDOC срез 25), но дефект не PI-специфичен — тот же
код путь используется для `new Comment()`/`new Text()`, так что
`div.appendChild(new Comment('x'))` на живой странице тоже ничего не
делает и не сообщает об этом. Любой скрипт (в том числе сторонние
библиотеки), вставляющий сконструированный через `new`/`document.create*`
detached CharacterData-узел в реальное дерево, теряет его молча.

## Что дальше

Архитектурный вопрос, не однострочная правка: детач-узлы сейчас
принципиально не имеют арена-представления («PIs are never laid out» —
комментарий у `_lumen_make_processing_instruction`), so a real fix means
either (a) promoting the detached node to a real arena node on insertion
(new arena node kind or reuse existing Comment/Text arena path — `data`
would need copying in, not sharing, breaking today's "one JS object = one
mutable string" identity), or (b) tracking parent/root purely in JS for
detached nodes without touching the arena/layout side at all (simpler, but
still lets a detached node sit *inside* an arena element's children list
inconsistently — arena-side `_lumen_get_children`/layout would not see it).
Не взят в GAP-XMLDOC/BUG-786 — не XML-специфично, и решение задевает больше
чем PI. Кандидат на отдельный P1/P3 срез с явным выбором (a) vs (b) до
правки.
