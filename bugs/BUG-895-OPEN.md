# BUG-895 — обёртка теневого корня — простой объектный литерал: у неё нет ни `ParentNode`-примеси (`append`/`prepend`/`replaceChildren`), ни прототипа вообще; у `document` `append`/`prepend` тоже нет

**Статус:** OPEN
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
