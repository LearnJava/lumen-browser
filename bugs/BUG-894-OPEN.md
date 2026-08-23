# BUG-894 — `Element.insertBefore` не проверяет, что опорный узел — ребёнок этого узла: `NotFoundError` не бросается никогда, а не-узел в опорном аргументе молча превращает вызов в `appendChild`

**Статус:** OPEN
**Заведён:** 2026-08-23 (WPT-RUN-6, срез 29 — живой замер, вариант `parentnode-mixin`)
**Область:** js (`crates/js/src/dom.rs:5066` — `insertBefore` элемента; сравните с `dom.rs:2526`, где у `document` та же проверка ЕСТЬ и бросает `NotFoundError`)
**Владелец:** P1/P3. Заведён P2 в ходе WPT-задачи, здесь не чинится.

## Симптом

DOM LS §4.2.3 «pre-insert» шаг 2 требует `NotFoundError`, если `child` не
является ребёнком `parent`. У элементной обёртки этой проверки нет вовсе:
`document.body.insertBefore(node, foreignNode)` вставляет узел и возвращает
его. У `document` — то же место реализовано правильно, так что дефект именно
в одной из двух копий.

Второй, тише: `if (!refNode || refNode.__nid__ === undefined) return
this.appendChild(newNode)` — то есть `insertBefore(node, "не-узел")`
не бросает `TypeError`, а добавляет в конец. `null` в этом аргументе — это
законный «в конец», а строка или число — нет.

Вставка `DocumentFragment` при этом корректна (оба ребёнка переезжают).

## Прямое измерение

`tests/wpt/verify_cssom_svg_interface_gaps.py --variant parentnode-mixin`
(2026-08-23, dev-release, Linux):

```
insertBefore-frag = 2          ← фрагмент разворачивается правильно
insertBefore-badarg = no-throw ← опорный узел из чужого поддерева принят
```

## Цена по WPT

`dom/nodes/Node-insertBefore.html` (остаток снимка WPT-RUN-5) — файл целиком
построен на проверках предусловий, и вместе с ним `ParentNode-append.html`
/`ParentNode-prepend.html`, подключающие общий
`dom/nodes/pre-insertion-validation-hierarchy.js`.

## Что дальше

Перенести проверку из `document.insertBefore` (`dom.rs:2526`) в элементную
и добавить ветку `TypeError` для не-узла — вместе это одна из двух половин
«pre-insertion validity»; вторая (иерархия: `Document` может принять лишь один
элемент, узел не может стать своим предком) в шиме тоже отсутствует и
проверяется тем же общим хелпером теста.
