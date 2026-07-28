# BUG-415 — отсоединённый документ (`createHTMLDocument`/`createDocument`/`new Document()`) не имеет ни методов `Node`, ни `head`/`body`

**Статус:** OPEN
**Компонент:** js (`crates/js/src/dom.rs:4728-4850` — `_lumen_build_detached_document`)
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
(`MAX_DOM_NODES`, см. [BUG-418](BUG-418-OPEN.md)). Отдельного `Document`-арены у
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
