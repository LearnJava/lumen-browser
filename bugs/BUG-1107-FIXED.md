# BUG-1107 — `document.importNode()` silently returns `null` for a node from `DOMParser().parseFromString(...)`

**Статус:** FIXED (P3, 2026-09-23), в срезе того же дня, что и заведение
**Заведён:** P3, 2026-09-23, попутно при локализации [BUG-791](BUG-791-FIXED.md) (dzen.ru главная)
**Компонент:** js (`crates/js/src/shim/web_api_shim_mid.js`)

## Симптом

Живой прогон `dzen.ru` (после хендшейка SSO, см. BUG-791 срез 2) показывает
в консоли 9 повторных `Uncaught TypeError: Cannot read properties of null
(reading 'childNodes')` внутри React-подобного микрофронтенда сайта, стек —
`t.render`/`t.mount` вокруг `useMemo`/React-scheduler (`MessagePort.
_onmessage` → `_lumen_tick_timers`). Один из главных контейнеров ленты
(`SECOND_CHUNK_APP_CONTAINER_MicroRoot`) в итоге остаётся пустым, хотя
сервер отдаёт его уже заполненным SSR-разметкой.

Разбор минифицированного бандла (`dzen-desktop-second.modern.bundle.js`,
`t.prototype.render`/`t.prototype.mount` рядом с `n.add`, SVG-иконки в
модуле «IconSet»):

```js
t.prototype.render = function() {
    var e = this.stringify();
    return (function(e) {
        var t = !!document.importNode,
            n = (new DOMParser).parseFromString(e, "image/svg+xml").documentElement;
        return t ? document.importNode(n, !0) : n;
    })(e).childNodes[0];
};
```

Это стандартный паттерн инлайнинга SVG-иконки: распарсить строку через
`DOMParser`, импортировать `documentElement` в живой документ через
`document.importNode`, взять первый дочерний узел. Изолированная проба
(`.tmp/bug791_svg_probe2.html`, не коммичена) подтвердила причину:
`document.importNode(el, true)` на элементе из `DOMParser`-документа
возвращал `null` **всегда**, независимо от корректности исходного SVG —
`.childNodes[0]` на этом `null` даёт ровно наблюдаемую ошибку.

## Причина

`document.importNode` (`crates/js/src/shim/web_api_shim_mid.js`) умел
клонировать только «нативные» узлы, у которых есть `__nid__` (backing в
Rust-арене):

```js
importNode: function(node, deep, options) {
    if (!node) return null;
    if (node.__nid__ !== undefined) { /* клонирование через арену */ }
    return null;   // ← любой другой Node молча превращался в null
},
```

Но `new DOMParser().parseFromString(...)` (`crates/js/src/dom_parser.rs`)
возвращает **виртуальный** документ — дерево из чистых JS-объектов
(`VElement`/`VText`/`VComment`/`VDocumentFragment`), у которых `__nid__` нет
и не может быть по конструкции (комментарий в `dom_parser.rs`: «The returned
Document is independent of the page DOM — it is backed by plain JS objects,
not Rust native nodes»). Любой `documentElement`/потомок такого дерева
попадал в ветку `return null`, вместо `TypeError` для не-`Node` аргумента
(DOM LS §4.7) или настоящего клона для `Node`-аргумента.

Реальные браузеры этот путь проходят штатно — расхождение чисто движковое,
а не поведение сайта: `secondChunkSyncExp`/пустой контейнер из BUG-791
среза 3 — не «сайт сам чистит контейнер», а следствие падения при первой
же попытке смонтировать иконку внутри ленты.

## Фикс

`importNode` получил вторую ветку: если у узла нет `__nid__`, но есть
`nodeType` (то есть это виртуальный узел из `dom_parser.rs`), дерево
материализуется в реальные арена-узлы новой функцией
`_lumen_materialize_virtual_subtree` — рекурсивный обход по
`nodeType`/`childNodes`/`getAttributeNames`/`getAttribute` (без `instanceof`,
классы `dom_parser.rs` приватны своему IIFE) через те же примитивы, что
`document.createElement`/`createTextNode`/`createComment`/
`createDocumentFragment` используют сами:
`_lumen_create_element`/`_lumen_create_element_ns`/`_lumen_create_text_node`/
`_lumen_create_comment`/`_lumen_create_fragment`/`_lumen_set_attr`/
`_lumen_append_child`.

Namespace виртуальный узел не хранит (документированный отдельный пробел в
`dom_parser.rs` — `namespaceURI` не резолвится из `xmlns`), поэтому фикс
берёт эвристику: если исходный документ распарсен с `contentType ===
"image/svg+xml"`, каждый элемент создаётся в SVG-namespace
(`_lumen_create_element_ns('http://www.w3.org/2000/svg', ...)`) — это
покрывает ровно сценарий инлайна SVG-иконок, самый частый повод звать
`DOMParser` + `importNode` вместе. Остальные contentType (`text/html`,
`application/xml`, …) используют обычный `createElement`, как и раньше —
за пределами SVG namespace-резолюция остаётся тем же документированным
пробелом, что и в `dom_parser.rs`.

`deep=false` теперь тоже соблюдается (раньше параметр не читался вовсе для
этой ветки, потому что её не существовало): верхний уровень копируется
всегда, потомки — только если `deep` истинно; рекурсивные вызовы дальше по
дереву всегда `deep`, поскольку решение «включать ли этого потомка» уже
принято на уровень выше.

## Известный смежный пробел (не в фиксе)

`_lumen_set_attr` (нативный биндинг) не сохраняет camelCase-регистр имени
атрибута для SVG-презентационных атрибутов — `viewBox` после материализации
читается в разметке как `viewbox` (`element.getAttribute('viewBox')` из JS
по-прежнему отдаёт то, что явно установлено, но `outerHTML`/`innerHTML`
печатает нижний регистр). Не влияет на сам крэш (иконка монтируется и
рендерится), не проверялось, влияет ли на реальный рендер SVG в Lumen —
отдельная локализация, не этот бага.

## Тест

`crates/js/src/dom/tests/v8_childnode_traversal.rs::document_import_node_materializes_dom_parser_svg_node`
— парсит `<svg><symbol id="foo" viewBox="…"><path …></symbol></svg>` через
`DOMParser`, импортирует `documentElement`, проверяет, что результат не
`null`, у него верный `tagName`, и что `.childNodes[0]` (сам символ) не
падает и отдаёт `id`/детей — воспроизводит ровно шаг, на котором падал
живой сайт.

## Замер

Изолированная проба до/после (`.tmp/bug791_svg_probe2.html`,
`.tmp/bug791_svg_probe3.html`, не коммичены, `lumen --dump-layout`):
до фикса — `document.importNode(el, true)` возвращает `null`; после —
возвращает реальный `<svg>` с материализованным потомком, `.appendChild`
в живой документ и `.innerHTML` отражают вставленный `<symbol>`.

`cargo clippy -p lumen-js --all-targets --features v8-backend -- -D
warnings` — чисто. `cargo test -p lumen-js --features v8-backend` — 151/151
(`scoped-test`-набор), полный `--lib` — 4172/4173 (единственный красный,
`credentials::tests::create_and_get_through_installed_provider`, падает
только при параллельном запуске и зелёный в изоляции — тестовая
неизоляция WebAuthn-тестов, не связано с этим фиксом, не трогалось).

## Связанные

* [BUG-791](BUG-791-FIXED.md) — заявка, в рамках которой найден; остаток
  заявки не закрыт этим фиксом (см. срез 5 там) — нужно проверить живьём,
  восстанавливает ли этот фикс полную ленту Дзена или падение было не
  единственной причиной пустого `SECOND_CHUNK_APP_CONTAINER_MicroRoot`.
