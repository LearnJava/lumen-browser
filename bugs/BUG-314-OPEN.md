# BUG-314: DOM-конструкторы не выставлены как глобальные интерфейсы

**Статус:** OPEN
**Дата:** 2026-07-18
**Компонент:** js (WEB_API_SHIM, `crates/js/src/dom.rs`)
**Найден:** P2-wpt S5, курируемый синхронный DOM-сабсет через `wptrunner`

## Симптом

Интерфейсы DOM-узлов не выставлены на глобальном объекте как конструкторы:
`Comment`, `Text`, `DocumentFragment`, `DocumentType`, `Document`,
`ProcessingInstruction`, а также `HTMLDivElement`/`HTMLElement` и прочие
`HTML*Element`. `grep -E 'function (Comment|DocumentFragment|DocumentType)'
crates/js/src/dom.rs` → пусто.

Наблюдаемые провалы:

- `DocumentFragment-constructor.html` → `DocumentFragment is not defined`.
- `Document-doctype.html` → `DocumentType is not defined`, `Document is not
  defined`.
- (вне курируемого сабсета, зафиксировано при прогоне) `Comment-constructor.html`
  и `Text-constructor.html` → `window[ctor] is not a constructor` для
  `new Comment()`/`new Text()` (тесты уходят в TIMEOUT).

Та же семья, что [BUG-305](BUG-305-OPEN.md) (`Image`/`HTMLImageElement`
конструктор отсутствует).

## Ожидание

DOM Standard / HTML: каждый интерфейс узла доступен как глобальный
конструктор с корректной цепочкой прототипов. Как минимум `new Comment(data)`,
`new Text(data)`, `new DocumentFragment()` создают соответствующие узлы;
`Document`/`DocumentType`/`ProcessingInstruction`/`HTML*Element` доступны для
`instanceof`-проверок. Реализовать в engine-agnostic `WEB_API_SHIM`.

## Воспроизведение

```bash
LUMEN_PROFILE=dev-release tests/wpt/.venv/Scripts/python.exe \
  tests/wpt/run_smoke.py /dom/nodes/DocumentFragment-constructor.html
```
