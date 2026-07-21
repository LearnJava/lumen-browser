# BUG-326: `CharacterData` interface incomplete/broken — no `length`/`substringData`/`appendData`/`insertData`/`deleteData`/`replaceData`; native Comment nodes misidentified as Text; `set_text_content` corrupts leaf Text/Comment writes

**Статус:** FIXED 2026-07-21
**Дата:** 2026-07-21
**Компонент:** js (WEB_API_SHIM, `crates/js/src/dom.rs`) + native `crates/js/src/v8_runtime.rs` (V8, default engine per ADR-018) + mirrored in `crates/js/src/dom.rs`'s QuickJS native bindings (rollback path)
**Найден:** реальная разработка по `docs/wpt-status.md` (WPT `dom/nodes/CharacterData-*.html` — `appendData` 1/14, `data` 0/16, `deleteData` 0/18, `insertData` 0/18, `replaceData` 0/34, `substringData` 1/28, `surrogates` 0/8), не заведённый ранее BUG.

## Симптомы (три независимых дефекта, одна WPT-таблица)

1. **`CharacterData.prototype` не имел ни одного из шести спек-методов** (DOM §4.10):
   `length`, `substringData`, `appendData`, `insertData`, `deleteData`, `replaceData` —
   грубый `grep` по всей кодовой базе не находил ни одного упоминания, кроме комментариев.
   Любой вызов падал как `TypeError: ... is not a function`.

2. **`document.createComment(data)` игнорировал аргумент и строил Text-узел, а не
   Comment** — двойной дефект в `_lumen_create_text_node`-based реализации
   (`crates/js/src/dom.rs`, обе точки: `document.createComment` и
   `_lumen_build_detached_document`'s `doc.createComment`). Хуже: **любой уже
   распарсенный из HTML `<!-- -->`-комментарий** (`crates/engine/html-parser/src/tree_builder.rs`
   уже вызывает настоящий `doc.create_comment` — эта часть была верной) тоже
   репортил `nodeType === 3` (TEXT_NODE, должно быть 8), `nodeName === '#text'`
   (должно быть `'#comment'`) и наследовал `Text.prototype` вместо
   `Comment.prototype` — потому что JS-обёртка (`_lumen_build_element`,
   `_lumen_is_text_node`) не различала `NodeData::Comment` от `NodeData::Text`
   вообще (не было биндинга `_lumen_is_comment_node`).

3. **`get_text_content`/`set_text_content` (native, `v8_runtime.rs` + mirrored
   `dom.rs`) реализовывали только Element/Document «textContent = replace all
   children with one Text node» семантику** — для настоящего **leaf**-узла
   (Text или Comment, у которого и так нет детей) `set_text_content` детачил
   пустых детей и добавлял **новый child-узел** под `id`, никогда не трогая
   собственную строку `id`. Следующее чтение возвращало старое (или склеенное)
   значение. `CharacterData.data`-сеттер, а значит и все шесть новых методов
   (все они в итоге пишут через `this.data = ...`), наследовали эту порчу.
   Плюс `get_text_content` на реальном Comment-узле возвращал `''` всегда
   (рекурсивный сборщик матчит только `NodeData::Text`, никогда `Comment`) —
   то есть чтение `.data`/`.textContent` любого native-комментария в дереве
   было пустой строкой независимо от реального содержимого.

## Фикс

**Native (`v8_runtime.rs`, V8 — дефолтный движок ADR-018; тот же фикс
зеркально применён и в QuickJS-путь `dom.rs`, чтобы rollback-сборка
(`--features quickjs`) не падала с `is not a function` на новом коде —
не новая инвестиция в quickjs, чисто механическое зеркалирование готового фикса):**

- `collect_text_content`: если `id` сам — `NodeData::Comment`, вернуть его
  строку напрямую (не запускать `collect_text_inner`, который намеренно
  матчит только `Text`-потомков — `Element.textContent` по спеке обязан
  игнорировать comment-потомков).
- `set_text_content`: если `id` сам — `NodeData::Text`/`NodeData::Comment`,
  просто перезаписать строку на месте (`*s = text.to_string()`) и вернуться —
  Element/Document-ветка (detach+rebuild) применяется только когда `id`
  реально может иметь детей.
- Новые нативные биндинги `_lumen_create_comment` (зовёт уже существовавший,
  но не подключённый к JS `Document::create_comment`) и `_lumen_is_comment_node`.

**Shared JS shim (`WEB_API_SHIM`, `dom.rs`):**

- `document.createComment(data)` (оба места — top-level `document` и
  `_lumen_build_detached_document`) теперь читает `data` и зовёт
  `_lumen_create_comment`, а не `_lumen_create_text_node('')`.
- `nodeType` getter (`_lumen_build_element`), MutationObserver's
  `_lumen_set_text_content`-wrap, `appendChild`'s CharacterData-receiver
  guard (BUG-325), `data`/`nodeValue` live-node accessor gate, и
  `[[Prototype]]` selection (BUG-322) — все расширены с
  `_lumen_is_text_node(nid)` на `_lumen_is_text_node(nid) ||
  _lumen_is_comment_node(nid)` (с раздельным разрешением 3 vs 8 и
  `Text.prototype` vs `Comment.prototype` где это различие важно).
  `TreeWalker`/`NodeIterator`'s `whatToShow` фильтр (`_nf_accepts`) заодно
  получил ветку `SHOW_COMMENT` (была объявлена в константах, но никогда не
  использовалась — `nt` мог быть только 1 или 3).
- `CharacterData.prototype.{length,substringData,appendData,insertData,
  deleteData,replaceData}` реализованы один раз на общем прототипе — работают
  для live Text/Comment-узлов, detached `new Comment()`/`new Text()`
  (BUG-314) и `ProcessingInstruction` (BUG-325) одинаково, поскольку все три
  формы дают `this.data` как собственный accessor, а методы построены только
  поверх него (offset/count через WebIDL `ToUint32` — `>>> 0` — JS-строки уже
  UTF-16-code-unit-индексированы, отдельная Rust-обвязка для суррогатных пар
  не нужна).

## Верификация

Юнит-тесты (`crates/js/src/dom.rs`, рядом с `character_data_prototype_chain`):
`create_comment_is_a_real_comment_node`, `live_text_and_comment_data_mutates_in_place`
(регрессия на дефект 3), `character_data_methods_spec_examples` (примеры из
спеки для все шести методов + `IndexSizeError`). `cargo test -p lumen-js --lib`:
2345/2345 зелёных (оба движка). `cargo clippy -p lumen-js --all-targets -- -D warnings`
(и с `--features v8-backend`, и без) — чисто.

Реальный прогон WPT (`tests/wpt/run_smoke.py`, dev-release, V8) на целевых 8 файлах:

| Тест | До | После |
|---|---|---|
| `CharacterData-appendData.html` | 1/14 | 12/14 |
| `CharacterData-data.html` | 0/16 | 14/16 |
| `CharacterData-deleteData.html` | 0/18 | **18/18** |
| `CharacterData-insertData.html` | 0/18 | **18/18** |
| `CharacterData-replaceData.html` | 0/34 | **34/34** |
| `CharacterData-substringData.html` | 1/28 | 26/28 |
| `CharacterData-remove.html` | 4/12 | 4/12 (не тронуто — `remove()`/`ChildNode` баг, отдельная причина) |
| `CharacterData-surrogates.html` | TIMEOUT 0/8 | TIMEOUT (не тронуто — зависает по другой причине, не исследовано) |

+120 сабтестов на 6 из 8 целевых файлов. Оставшиеся гэпы:
`substringData()`/`insertData()` и т.п. без обязательных аргументов должны
бросать `TypeError` (WebIDL arity check), сейчас тихо трактуют `undefined` как
0 — 2 сабтеста в `substringData`/аналогично, вероятно, в `appendData`/`data`
(не заведено отдельно, тривиальный follow-up). `CharacterData-remove.html` и
`-surrogates.html` — независимые баги вне скоупа этого фикса.
