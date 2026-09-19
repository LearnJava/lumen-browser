# BUG-895 — обёртка теневого корня — простой объектный литерал: у неё нет ни `ParentNode`-примеси (`append`/`prepend`/`replaceChildren`), ни прототипа вообще; у `document` `append`/`prepend` тоже нет

**Статус:** FIXED 2026-09-19
**Заведён:** 2026-08-23 (WPT-RUN-6, срез 29 — живой замер, вариант `parentnode-mixin`)
**Область:** js (`crates/js/src/dom.rs:1577` — `_lumen_make_shadow_root` собирает `var sr = { ... }` без прототипа; `dom.rs` — литерал `document` без `append`/`prepend`/`replaceChildren`)
**Владелец:** P1/P3. Заведён P2 в ходе WPT-задачи, здесь не чинится.

## Симптом

`shadowRoot.append(node)` — `TypeError: root.append is not a function`. У
элемента и у `DocumentFragment` та же примесь работает (`append`, `prepend`,
`replaceChildren`, `before`/`after`/`replaceWith`/`remove`), у `document` —
`append` отсутствует. Причина одна: теневой корень собирается литералом с
поимённо перечисленными методами (`appendChild`, `querySelector`, …), а не
через прототипную цепочку, поэтому всё, что добавляют в `Element.prototype`,
мимо него проходит.

Тот же литерал — причина [BUG-877](BUG-877-OPEN.md) (`host.shadowRoot !==
host.shadowRoot`: обёртка создаётся заново на каждое чтение), так что чинится
это одним изменением: сделать теневой корень настоящим узлом с прототипом и
кэшировать его.

## Прямое измерение

`tests/wpt/verify_cssom_svg_interface_gaps.py --variant parentnode-mixin`
(2026-08-23, dev-release, Linux):

```
element-append = 3        element-prepend = z      element-replaceChildren = only
fragment-append = 1
document-append = undefined
shadow-append THREW root.append is not a function
child-before = function   child-after = function   child-replaceWith = function
```

## Цена по WPT

2 id снимка WPT-RUN-5 с текстом `div.shadowRoot.append is not a function`:
`the-dialog-element/dialog-focus-shadow-double-nested.html` и
`the-dialog-element/dialog-focus-shadow.html`. Плюс
`dom/nodes/ParentNode-append.html`/`-prepend.html`, где `document` — один из
четырёх проверяемых узлов (эти два механизм
`insertbefore-no-validation`/[BUG-894](BUG-894-OPEN.md) забирает раньше, как
причину, которая срабатывает первой).

## Что дальше

Собрать теневой корень как объект с прототипом (`ShadowRoot.prototype` →
`DocumentFragment.prototype` → `Node.prototype`) и кэшировать по nid; примесь
`ParentNode` тогда достаётся и ему, и `document`, куда её надо добавить
отдельной строкой.

## Исправлено 2026-09-19 (P3)

Прототипная цепочка `ShadowRoot.prototype → DocumentFragment.prototype →
Node.prototype` (`crates/js/src/shim/web_api_shim_mid.js`) уже была собрана
отдельным более ранним фиксом BUG-676 — но ни `DocumentFragment.prototype`,
ни `Node.prototype` никогда не несли `ParentNode`-примесь: `append`/`prepend`/
`replaceChildren` жили только в объектном литерале элемента и в отдельном
ad hoc литерале `DocumentFragment`, ни через один из которых `ShadowRoot` не
проходит. Добавлены `ShadowRoot.prototype.append`/`.prepend`/`.replaceChildren`
— тот же алгоритм строка-в-текстовый-узел/множественные аргументы, что у
элемента. Отдельно на `document` (объектный литерал, у которого уже были
`appendChild`/`insertBefore`/`removeChild`/`replaceChild` от BUG-557) добавлены
`append`/`prepend`/`replaceChildren`, делегирующие в эти же Node-методы.
Кэширование обёртки (второе условие "чинится одним изменением" из этого бага)
закрыто вместе с [BUG-877](BUG-877-FIXED.md) в том же коммите — без него
`ShadowRoot.prototype`-методы работали бы, но на новой обёртке каждый раз.
Вне скоупа: `firstChild`/`lastChild`/`children`-соседи (`childElementCount`,
`firstElementChild`, ...) на `ShadowRoot` — `children` уже был, остальные не
запрошены симптомом этого бага. Регресс-тесты
`shadow_root_has_parentnode_append_prepend_replace_children`,
`document_has_parentnode_append_prepend_replace_children`,
`document_append_and_prepend_grow_document_child_nodes`
(`crates/js/src/dom/tests/v8_bug877_895_shadow_root_wrapper.rs`). Гейты:
`cargo clippy -p lumen-js --all-targets --features v8-backend -- -D warnings`
чисто, `cargo test -p lumen-js --features v8-backend` 3897/3899 (см. BUG-877
для двух предсуществующих флаков, не связанных с этим фиксом).
