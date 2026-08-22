# BUG-415 — отсоединённый документ (`createHTMLDocument`/`createDocument`/`new Document()`) не имеет ни методов `Node`, ни `head`/`body`

**Статус:** FIXED 2026-08-22 (P3)
**Компонент:** js (`crates/js/src/dom.rs` — `_lumen_build_detached_document`)
**Найден:** 2026-07-28 (P2), WPT-VENDOR-html, срез `html/dom`

## Симптом

Проба (`--dump-layout`, `var d = document.implementation.createHTMLDocument('x')`):

```
newdoc.appendChild=function | newdoc.removeChild=undefined
newdoc.head=undefined       | newdoc.body=undefined
```

`appendChild` есть — но это не `Node.appendChild`, а локальная функция, кладущая узел в
JS-массив `_children` (`dom.rs:4785-4792`). Всё остальное семейство мутаций
(`removeChild`, `insertBefore`, `replaceChild`, `cloneNode`, `contains`, `hasChildNodes`)
и HTML-аксессоры (`head`, `body`, `title`, `getElementById`, `getElementsByTagName`,
`querySelector`) не определены вовсе.

## Отношение к BUG-358 — это обратная сторона того же раскола

[BUG-358](BUG-358-OPEN.md) зафиксировал, что **живой** `document` не имеет
document-metadata-атрибутов (`characterSet`/`compatMode`/`URL`/…), которые
`_lumen_build_detached_document` как раз определяет. Здесь ровно наоборот: отсоединённый
документ не имеет `Node`-интерфейса и HTML-аксессоров, которые у живого `document` есть.
Два независимо написанных объекта документа с непересекающимися дырами — тот же дефект
архитектуры шима, что BUG-358 описывает со своей стороны; чинить их разумно вместе
(один общий строитель + различия), поэтому баг заводится отдельным номером, а не
дописывается в BUG-358.

## Данные WPT

Срез `html/dom` (`run_report.py --all --root html/dom --recursive`):

| Файл | Сабтесты | Доминирующее сообщение |
|---|---|---|
| `documents/dom-tree-accessors/Document.body.html` | 2/26 | `doc.removeChild is not a function` (17 из 24 FAIL) |

`Document.body.html` — канонический тест выбора `body`-элемента (`<body>` vs `<frameset>`,
не-HTML-namespace, отсутствие корня); он строит документы через
`document.implementation.createHTMLDocument('')`, чистит их `doc.removeChild(...)` и читает
`doc.body`. Первый же вызов `removeChild` валит 17 сабтестов до их собственного утверждения —
то есть **сам предмет теста (`document.body`) в срезе фактически не проверен**; цифра
2/26 занижает, а не завышает разрыв. Ещё 2 сабтеста падают по другой причине
(`assert_throws_js`/`assert_throws_dom` — сеттер `document.body` не валидирует аргумент),
1 — `HTMLFrameSetElement is not defined`.

Родственные `readyState`-сабтесты того же среза (`Document.readyState` для документов из
`createHTMLDocument()`, `DOMParser`, `createDocument()` — 4 FAIL) упираются в ту же дыру:
`readyState` строителем тоже не определён.

## Смежное наблюдение (в баг не выделяется)

`createHTMLDocument` строит скелет `html>head,body` через `_lumen_create_element`, то есть
узлы отсоединённого документа выделяются из **той же общей арены**, что и живое дерево
(`MAX_DOM_NODES`, см. [BUG-418](BUG-418-FIXED.md)). Отдельного `Document`-арены у
отсоединённых документов нет — это уже задокументировано в doc-комментарии над
`_lumen_build_detached_document` («has no per-node document tag — a known simplification»),
но стоит держать в виду при починке: добавление `removeChild` без владельца-арены даст
узлы-сироты, а не освобождение.

## Направление починки

Свести живой `document` и `_lumen_build_detached_document` к одному строителю: общий набор
`Node`/`ParentNode`/`Document`-членов поверх реального арены-поддерева (узлы уже настоящие —
`_lumen_make_element`, с `__nid__`), а различия (`location`, `URL`, `readyState`,
`contentType`) — параметрами. Тогда закрываются обе стороны раскола: и эта, и
[BUG-358](BUG-358-OPEN.md).

## Частично закрыто (P3, 2026-08-09, в ходе разбора [BUG-703](BUG-703-FIXED.md))

`head` и `body` у отсоединённого документа реализованы: `_lumen_build_detached_document`
определяет их как первый элемент-потомок `documentElement` с тегом `HEAD` и
`BODY`/`FRAMESET` соответственно (HTML LS 3.1.4), поверх того же реального
арена-поддерева, по которому уже ходит `documentElement`. Тест —
`detached_document_exposes_head_and_body` в `dom::tests::v8_core`.

Остаётся всё остальное из симптома: `removeChild`/`insertBefore`/`replaceChild`/
`cloneNode`/`contains`/`hasChildNodes`, `title`, `readyState`,
`getElementById`/`getElementsByTagName`/`querySelector`. Именно `removeChild`
даёт 17 из 24 FAIL в `Document.body.html`, так что вклад этого фикса в цифры
WPT, скорее всего, нулевой до того, как появятся методы `Node`. Направление
починки (общий строитель для живого и отсоединённого документа) не изменилось.

## Закрыто (P3, 2026-08-22)

Остаток симптома закрыт в `_lumen_build_detached_document` — **без** сведения к одному
строителю с живым `document`: это переписывание всего объекта документа (домен P1,
BUG-358 со своей стороны остаётся OPEN), а здесь нужен был работающий `Node`-интерфейс
поверх уже существующего списка `_children`.

Что появилось:

- **Мутации `Node` над собственным списком детей документа.** `insertBefore`/`removeChild`/
  `replaceChild` + переписанный `appendChild`. Всё, что *ниже* документного элемента, —
  обычное арена-поддерево и уже мутировало через обёртки элементов; своего арена-хранения
  не имело ровно ребро «документ → ребёнок», оно живёт в `_children`. Общий пре-вставочный
  шаг (DOM 4.2.3) вынесен в `_detached_adopt`: узел снимается и с арена-родителя, и из этого
  списка, прежде чем вставляться. Поиск ребёнка (`_detached_child_index`) сравнивает сперва
  по идентичности обёртки, потом по `nid` — обёртка арена-узла чеканится заново на каждом
  обращении, поэтому одной идентичности мало.
- **`ParentNode`/`Node`-аксессоры:** `hasChildNodes`, `firstChild`/`lastChild`,
  `children`/`childElementCount`/`firstElementChild`/`lastElementChild`, `contains`
  (унаследованный `Node.prototype.contains` ходит по `parentNode`, а у ребёнка документа
  этой связи нет — мостится ровно один недостающий шаг), `cloneNode(deep)`.
- **`readyState`** — всегда `'complete'` (HTML LS 3.1.5: документ без browsing context
  ничего не грузит).
- **Древесные аксессоры, ограниченные поддеревом документного элемента:**
  `querySelector`/`querySelectorAll`/`getElementById`/`getElementsByTagName`/
  `getElementsByClassName`. `id` и имя тега ищутся обходом, а не через селекторный движок:
  это произвольный текст, и экранирование его в селектор превратило бы промах поиска в
  ошибку разбора.
- **`title`** — геттер (child text content, схлопнутый пробел) и сеттер (перенацелить
  существующий `title`, иначе создать в `head`; без `head` — no-op, как требует спец).
- **Сеттер `document.body`** (HTML LS 3.1.4): принимает только `body`/`frameset`
  (иначе `HierarchyRequestError`, не-узел — `TypeError`), заменяет текущий body на месте
  или дописывает к документному элементу.
- **`HTMLFrameSetElement`** — интерфейса не было вовсе; добавлен в список конструкторов и
  в `_lumen_html_tag_prototypes` (`FRAMESET`).

Отдельно исправлена **область видимости `head`/`body`**: оба аксессора обязаны быть
укоренены в *html-элементе*, которым по HTML LS 3.1.4 документный элемент является только
если это `html` в HTML-пространстве имён. До этого обход начинался с любого документного
элемента, поэтому `doc.appendChild(createElement('body'))` + `body.appendChild(frameset)`
отдавал frameset как `doc.body`. Тот же обход в сеттере, наоборот, дописывает к
документному элементу как таковому (спец так и требует; геттер после этого продолжает
отдавать `null`).

## Проверка

Четыре юнит-теста в `dom::tests::v8_core` (`crates/js/src/dom.rs`):
`detached_document_has_node_mutation_members`, `detached_document_body_scope_and_accessors`,
`detached_document_body_setter` и порт `doc`-стороны самого WPT-теста —
`detached_document_wpt_document_body_subtests` (каждый `assert_equals` оригинала = одна
клауза, результат — индекс первого `false`).

```
cargo test -p lumen-js --features v8-backend detached_document
```

**Четыре сабтеста порта намеренно исключены** — те, что кладут элемент в чужое
пространство имён и ждут, что он останется отличим от HTML-одноимённого. Арена хранит
`Namespace` шестизначным перечислением, поэтому `createElementNS('http://example.org/test',
'body')` неотличим от `createElement('body')`. Это [BUG-830](BUG-830-OPEN.md) — отдельный
дефект слоем ниже, заведён в ходе этой починки.

Что осталось за рамками: общий строитель для живого и отсоединённого документа
([BUG-358](BUG-358-OPEN.md), [BUG-557](BUG-557-OPEN.md)) — раскол объектов документа как
таковой не устранён, закрыта только эта его сторона.
