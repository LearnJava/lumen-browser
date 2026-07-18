# BUG-310: ElementTraversal и ParentNode.children отсутствуют в DOM-шиме

**Статус:** OPEN
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
