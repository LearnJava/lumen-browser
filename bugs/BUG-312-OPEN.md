# BUG-312: Element.hasAttributes() отсутствует в DOM-шиме

**Статус:** OPEN
**Дата:** 2026-07-18
**Компонент:** js (WEB_API_SHIM, `crates/js/src/dom.rs`)
**Найден:** P2-wpt S5, курируемый синхронный DOM-сабсет через `wptrunner`

## Симптом

`Element.prototype.hasAttributes()` (DOM Standard §4.9.2) отсутствует —
`grep hasAttributes crates/js/src/dom.rs` → 0 совпадений.

Провалы сабтестов `Element-hasAttributes.html` (оба `expected: FAIL`):

```
must return false when the element does not have attributes
  -> buttonElement.hasAttributes is not a function
must return true when the element has attribute
  -> divWithId.hasAttributes is not a function
```

## Ожидание

`hasAttributes()` → `true`, если у элемента есть хотя бы один атрибут.
Тривиально поверх уже существующей модели атрибутов (`hasAttribute`/
`getAttribute` есть). Реализовать в engine-agnostic `WEB_API_SHIM`.

## Воспроизведение

```bash
LUMEN_PROFILE=dev-release tests/wpt/.venv/Scripts/python.exe \
  tests/wpt/run_smoke.py /dom/nodes/Element-hasAttributes.html
```
