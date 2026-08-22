# BUG-830 — произвольный namespace-URI в `createElementNS` схлопывается в HTML

**Статус:** OPEN
**Компонент:** dom (`crates/engine/dom/src/lib.rs:119` — `enum Namespace`), js
(`crates/js/src/v8_runtime.rs:2372-2394` — натив `_lumen_create_element_ns`)
**Найден:** 2026-08-22 (P3), при закрытии [BUG-415](BUG-415-FIXED.md) — четыре сабтеста WPT
`html/dom/documents/dom-tree-accessors/Document.body.html` не сходились после того, как всё
остальное в файле заработало

## Симптом

```js
var e = document.createElementNS('http://example.org/test', 'body');
e.namespaceURI  // 'http://www.w3.org/1999/xhtml'  — ожидается 'http://example.org/test'
e.tagName       // 'BODY'                          — ожидается 'body'
e.localName     // 'body'
```

Элемент в произвольном (не SVG, не пустом) пространстве имён неотличим от HTML-элемента с тем
же локальным именем: у него HTML-овский `namespaceURI`, апперкейснутый `tagName` и HTML-овский
прототип. Измерено пробой через `v8_runtime_with_dom` на текущем `main`.

## Корень

`Namespace` (`crates/engine/dom/src/lib.rs:119`) — перечисление из шести известных пространств
имён плюс `None`; произвольного URI в нём представить нечем. Натив
`_lumen_create_element_ns` поэтому сводит вход к трём случаям:

```rust
let namespace = if ns == "http://www.w3.org/2000/svg" {
    Namespace::Svg
} else if ns.is_empty() {
    Namespace::None
} else {
    Namespace::Html      // <- сюда попадает ЛЮБОЙ другой URI
};
```

Обратное отображение (`_lumen_get_namespace_uri`) отдаёт канонический URI варианта, отсюда
подменённый `namespaceURI`; `_lumen_qualified_tag_name` апперкейсит имя именно по признаку
«namespace === XHTML», отсюда `BODY` вместо `body`.

## Отношение к соседним багам

- [BUG-328](BUG-328-FIXED.md) закрыл ровно один частный случай этого же схлопывания —
  `createElementNS("", …)`, ради чего и появился вариант `Namespace::None`. Произвольный URI
  тогда не рассматривался.
- [BUG-685](BUG-685-OPEN.md) — про **парсер**: он вообще не реализует foreign content и
  штампует `QualName::html` на всё, включая содержимое `<svg>`. Это другая дорога к дереву;
  здесь ломается скриптовая, где namespace передан явно и всё равно теряется.
- Общий знаменатель у обоих — узость самого `Namespace`; MathML в перечислении есть, а
  расширяемого варианта «прочий URI» нет.

## Цена

WPT `html/dom/documents/dom-tree-accessors/Document.body.html`: четыре сабтеста
(«Body followed by frameset inside a non-HTML html element», «Frameset followed by body inside
a non-HTML html element», «Non-HTML body followed by body inside the html element», «Non-HTML
frameset followed by body inside the html element») не могут пройти, пока движок не различает
пространства имён. Они намеренно исключены из порта этого файла в юнит-тест
`detached_document_wpt_document_body_subtests` (`crates/js/src/dom.rs`) — остальные тринадцать
`doc`-сабтестов там проходят.

Шире: любой тест, строящий foreign-content дерево через `createElementNS` с чужим URI, и любой
селектор/аксессор, который обязан отличать HTML-элемент от одноимённого чужого.

## Направление починки

Заменить `Namespace` на тип, у которого есть вариант с произвольной строкой (например
`Namespace::Other(Arc<str>)` рядом с существующими быстрыми вариантами, чтобы сравнение
известных пространств осталось дешёвым). Затрагивает `QualName` и всех его потребителей —
селекторы, layout-конструирование, a11y, сериализация — поэтому это не точечная правка шима,
а правка ядра DOM; чинить разумно вместе с [BUG-685](BUG-685-OPEN.md), которому нужен тот же
расширенный тип для таблиц HTML LS §13.2.6.5.

## Как воспроизвести

```
cargo test -p lumen-js --lib --features v8-backend detached_document_wpt
```
проходит без четырёх исключённых сабтестов; вернуть их в скрипт теста — и он покажет
`r.indexOf(false)` на первом же namespace-зависимом.
