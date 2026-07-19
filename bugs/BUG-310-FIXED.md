# BUG-310: ElementTraversal и ParentNode.children отсутствуют в DOM-шиме

**Статус:** FIXED 2026-07-19
**Дата:** 2026-07-18
**Компонент:** js (WEB_API_SHIM, `crates/js/src/dom.rs`)
**Найден:** P2-wpt S5, курируемый синхронный DOM-сабсет через `wptrunner` (`tests/wpt/metadata/dom/nodes/`)

## Симптом

Интерфейс ElementTraversal (DOM Standard §4.2.7) и `ParentNode.children`
(§4.2.6) целиком отсутствуют в основном шиме. `grep -E
'childElementCount|firstElementChild|lastElementChild|nextElementSibling|previousElementSibling'
crates/js/src/dom.rs` → 0 совпадений.

Наблюдаемые провалы сабтестов (все `expected: FAIL` в метадате):

- `Element-childElementCount.html`, `Element-childElementCount-nochild.html`
  → `assert_equals: expected 0 but got undefined` (`childElementCount`).
- `Element-firstElementChild.html`, `Element-lastElementChild.html`,
  `Element-nextElementSibling.html`, `Element-previousElementSibling.html`,
  `Element-childElement-null.html`, `Element-siblingElement-null.html`
  → `assert_true: expected true got false` (свойства возвращают `undefined`).
- `Element-children.html` → `container.children.item is not a function`
  (`.children` — это голый массив без семантики `HTMLCollection`/`.item()`).
- `ParentNode-children.html` → `Cannot read properties of undefined (reading
  'children')` (`.children` отсутствует на узле).

## Ожидание

На `Element.prototype` (и `Document`/`DocumentFragment` для `children`):
`children` (живой `HTMLCollection` с `.item()`/индексами — в нашем шиме
допустим статический массиво-подобный объект с `.item`, как соседние
коллекции), `childElementCount`, `firstElementChild`, `lastElementChild`;
на `Element.prototype`: `nextElementSibling`, `previousElementSibling`.
Реализовать в engine-agnostic `WEB_API_SHIM` — та же семья пробелов, что
[BUG-299](BUG-299-FIXED.md)/[BUG-302](BUG-302-OPEN.md).

## Воспроизведение

```bash
LUMEN_PROFILE=dev-release tests/wpt/.venv/Scripts/python.exe \
  tests/wpt/run_smoke.py /dom/nodes/Element-childElementCount.html
# subtest FAIL: assert_equals expected 0 but got undefined
```

Флип `expected: FAIL` → `PASS` в соответствующих `.ini` после реализации.

## Решение (2026-07-19)

Всё реализовано в engine-agnostic `WEB_API_SHIM` (общий для V8 и rquickjs):

- Хелперы `_lumen_is_element_nid` (элемент = не text-узел и tag-name не
  начинается с `#`) и `_lumen_element_child_nids` (element-only дети в
  tree-order) — фильтруют text/comment-узлы, которые `_lumen_get_children`
  возвращает наравне с элементами.
- Аксессоры на элементе: `childElementCount`, `firstElementChild`,
  `lastElementChild`, `nextElementSibling`, `previousElementSibling`
  (последние два ищут позицию узла среди element-детей родителя и берут
  соседа, перескакивая text-узлы).
- `Node.parentNode` — раньше на обёртках элементов был ТОЛЬКО `parentElement`,
  `parentNode` отсутствовал вовсе (отсюда `Cannot read properties of undefined
  (reading 'children')` в `ParentNode-children.html`). Добавлен зеркально
  `parentElement`.
- `children` — теперь живой `HTMLCollection` (`_lumen_make_html_collection`)
  на базе `Proxy`: `length`/индексы/`item(i)`/`namedItem(name)` перезапрашивают
  живое дерево при каждом обращении. Прото — маркер `HTMLCollection`
  (выставлен глобально как `window.HTMLCollection`) для `instanceof`. То же
  для `DocumentFragment.children`.

Юнит-тесты (rquickjs, общий шим): `element_traversal_*`,
`parent_node_children_is_reachable`, `children_is_live_html_collection`,
`children_collection_item_named_and_index`.

`.ini` флипнуты в PASS для 8 ElementTraversal-файлов + `ParentNode-children`.

**Остаток (НЕ в этом фиксе):** `Element-children.html` остаётся `FAIL` — его
два сабтеста требуют полной enumeration-семантики `HTMLCollection`
(точный порядок `Object.getOwnPropertyNames`, non-enumerable именованные
свойства) И зависят от `createElementNS("", ...)`, дающего элемент вне
HTML-namespace; Lumen сейчас сворачивает любой не-SVG namespace в HTML
(`_lumen_create_element_ns`), поэтому правило экспонирования по `name`
невыполнимо. Обе причины — вне ElementTraversal-скоупа BUG-310.
