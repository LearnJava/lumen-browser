# BUG-327: `Node.prototype.hasChildNodes()` missing entirely; ordinary live element/text/comment wrapper has no `.childNodes` at all

**Статус:** FIXED 2026-07-21
**Дата:** 2026-07-21
**Компонент:** js (WEB_API_SHIM, `crates/js/src/dom.rs`, shared by both engines — no native-side change needed)
**Найден:** реальная разработка по `docs/wpt-status.md` (WPT `dom/nodes/Document-createTextNode.html`, `Document-createComment-createTextNode.js` — `c.hasChildNodes is not a function`), не заведённый ранее BUG.

## Симптомы

1. **`Node.prototype.hasChildNodes()` не существовал вовсе** — `grep` по всей
   кодовой базе не находил ни одного упоминания `hasChildNodes`, кроме этого
   фикса. Любой вызов падал как `c.hasChildNodes is not a function`.

2. **`.childNodes` отсутствовал на обычной live-обёртке узла**
   (`_lumen_build_element`, обслуживает и элементы, и live Text/Comment) —
   было реализовано только для `document` (собственный геттер),
   `DocumentFragment`, detached `CharacterData` (`new Text()`/`new Comment()`,
   всегда пустой массив) и `ShadowRoot`-подобных `children` (element-only, не
   `childNodes`). Обычный `document.createElement('div').childNodes` был
   `undefined`, а не `NodeList`-подобным массивом — независимо от
   `hasChildNodes()`, любой код, обходящий live-поддерево через `.childNodes`
   (включая `Node.prototype.hasChildNodes()`, добавленный этим же фиксом),
   тоже был сломан.

   Обнаружено при живой отладке через `.tmp/debug_probe.py` (BiDi
   `script.evaluate` против реально запущенного `lumen --bidi-port`,
   аналог `tests/wpt/verify_s6_await_promise.py`): первая попытка
   диагностики завела в сторону («instanceof Text/Node ложны в реальном
   пайплайне») — оказалось артефактом **устаревшего `target/dev-release/lumen.exe`**
   (собран 2026-07-20, до мержа BUG-322/BUG-326 в тот же день); после
   пересборки инстансы instanceof оказались верны, а настоящей причиной был
   именно отсутствующий `hasChildNodes`/`childNodes`. См. известную ловушку
   в `CLAUDE.md` → "run.py stale binary" — тот же паттерн применим и к
   ручному BiDi-прогону, не только к `graphic_tests/run.py`.

## Фикс

**`crates/js/src/dom.rs` (`WEB_API_SHIM`, движко-независимый, применяется
к обоим движкам без отдельного зеркалирования):**

- `Node.prototype.hasChildNodes = function() { return this.childNodes.length > 0; }`
  — добавлено сразу после `function Node() {...}`, работает для любого узла,
  чей `[[Prototype]]` восходит к `Node.prototype` (element/text/comment через
  BUG-322, `Document`/`DocumentFragment`/`DocumentType` через их собственные
  `Object.create(Node.prototype)`), при условии что у самого узла есть
  `.childNodes`.
- `childNodes`-геттер добавлен в `_obj` (`_lumen_build_element`), рядом с
  `firstChild`/`lastChild`: `_lumen_get_children(nid).map(_lumen_make_element)`
  — закрывает и элементы, и live Text/Comment одним изменением (для
  текстовых/comment-узлов `_lumen_get_children` естественно пустой).
- `childNodes: []` добавлен в `_lumen_make_doctype`'s `obj` — `DocumentType`
  по спеке никогда не имеет детей, но теперь как минимум не роняет
  `hasChildNodes()`.
- `document`-синглтон получил собственный `hasChildNodes` (не наследуется —
  `document` объект НЕ подключён к `Document.prototype` через
  `Object.setPrototypeOf`, отдельный, более широкий гэп вне скоупа этого
  фикса).

## Верификация

Новый юнит-тест `node_child_nodes_and_has_child_nodes`
(`crates/js/src/v8_runtime.rs`, рядом с `query_selector_finds_element_by_id`):
элемент с двумя live-детьми (`hasChildNodes()===true`, `childNodes.length===2`,
`childNodes[0]`/`[1]` === те же обёртки), лист-элемент/text/comment
(`hasChildNodes()===false`, `childNodes.length===0`), `document.hasChildNodes()`.
`cargo test -p lumen-js --features v8-backend`: все тесты зелёные.
`cargo clippy -p lumen-js --features v8-backend --all-targets -- -D warnings`
— чисто.

Реальный прогон WPT (`tests/wpt/run_smoke.py`, dev-release, V8) после
пересборки бинаря:

| Тест | До | После |
|---|---|---|
| `Document-createTextNode.html` | 0/6 | **6/6** |

`Document-createComment.html` осталась 5/6 (независимый баг:
`createComment(undefined)` → `data` должен быть строка `"undefined"`, а не
`""` — вне скоупа этого фикса, не заведён отдельно). `Node-childNodes.html`,
`Node-removeChild.html` затрагивают тот же `.childNodes`, но упираются в
отдельные, гораздо более крупные гэпы (живой `NodeList`-класс/кэширование,
iframe/synthetic-document поддержка) — не тронуты, отдельная задача.
