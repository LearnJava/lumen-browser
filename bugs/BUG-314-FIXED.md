# BUG-314: DOM-конструкторы не выставлены как глобальные интерфейсы

**Статус:** FIXED 2026-07-20
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

## Решение (2026-07-20, P3)

Новый блок «DOM interface constructors» в `WEB_API_SHIM` (`crates/js/src/dom.rs`):

- **Интерфейс-глобалы** для reference/`instanceof`-резолюции (раньше бросали
  `X is not defined`): `Node`, `Element`, `CharacterData`, `Attr`, `Document`,
  `DocumentType`, `ProcessingInstruction`, `HTMLElement` (function-декларации,
  hoisted-глобалы) + генерируемый через `globalThis[name]` набор из ~36
  `HTML*Element` (с гардом `name in globalThis`, чтобы не затирать существующие
  `HTMLImageElement`/`Image`). Прототипы связаны: `Element.prototype` →
  `Node.prototype`, `HTMLElement.prototype` → `Element.prototype`,
  `HTML*Element.prototype` → `HTMLElement.prototype`.
- **Конструируемые узлы**: `new Comment(data)` / `new Text(data)` строят detached
  CharacterData-объекты через `_lumen_make_character_data(nodeType, nodeName,
  data, proto)` с полной цепочкой прототипов (`Comment.prototype` →
  `CharacterData.prototype` → `Node.prototype`) и рабочим `instanceof`; `data`
  стрингифицируется по DOM §4.5 (undefined → `''`, читается только первый
  аргумент). `new DocumentFragment()` возвращает нативный (arena-backed) пустой
  фрагмент; обёртке `_lumen_make_document_fragment` добавлены `ownerDocument` и
  `firstChild`.
- PI-узлу (`_lumen_make_processing_instruction`, BUG-313) выставлен прототип
  `ProcessingInstruction.prototype`, так что `pi instanceof
  ProcessingInstruction`/`CharacterData`/`Node` теперь истинны.

Юнит-тесты в `crates/js/src/dom.rs`: `comment_text_constructors_build_nodes`,
`character_data_prototype_chain`, `document_fragment_constructor`,
`dom_interface_globals_defined`.

**Отложено в [BUG-321](BUG-321-FIXED.md):** конструируемый `new Document()`
(с `createElement`/`appendChild`/`doctype`), живой `document.doctype` как
`DocumentType`-узел и `instanceof HTML*Element` для нативных элемент-обёрток
(общий долг, трекается как [BUG-322](BUG-322-FIXED.md) — обёртки остаются
plain-объектами). Из-за этого
`Document-doctype.html` проходит частично.
