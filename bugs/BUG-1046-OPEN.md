# BUG-1046: `TreeWalker`/`NodeIterator` с корнем `document` не обходят ничего — `nextNode()` молча отдаёт `null` при любом `whatToShow`, а `firstChild()`/`lastChild()` рядом работают

**Статус:** OPEN
**Компонент:** js (`crates/js/src/shim/web_api_shim_mid_b.js` — `_TreeWalker.prototype._root_nid:5181`, `nextNode:5321`, `previousNode:5304`, `firstChild:5225`, `_NodeIterator.prototype._ensure:5349`, `_nf_accepts:5143`; фабрики — `crates/js/src/shim/web_api_shim_mid.js::createTreeWalker:9890` / `createNodeIterator:9894`)
**Найден:** P6, 2026-09-09, задача E2E-2 (проба «TreeWalker и SHOW_COMMENT»)

## Симптом

`document.createTreeWalker(document, …)` — самая частая форма вызова — не находит
**ни одного** узла. Не только комментарии: любой `whatToShow`, включая `SHOW_ALL`.

Проба (`--dump-layout` по http, минимальная страница с четырьмя комментариями:
один до `<html>`, три внутри `<body>`):

```
tw.document.COMMENT.count        = 0      ← ожидается 4
tw.document.ELEMENT.count        = 0      ← ожидается 9
tw.document.ALL.count            = 0
ni.document.COMMENT.count        = 0      ← NodeIterator тот же результат
ni.document.ELEMENT.count        = 0

tw.documentElement.COMMENT.count = 3      ← корень-элемент работает
tw.documentElement.ELEMENT.count = 8
tw.body.COMMENT.count            = 3
tw.innerHTMLhost.COMMENT.count   = 2      ← замер 2026-09-09 воспроизведён
```

Комментарии в дереве есть — ручной обход по `childNodes` от `document` находит
все четыре (`[" top-level-comment-before-html "," body-comment-1 ",
" nested-comment-2 "," body-comment-3 "]`), `document.childNodes.length === 3`
(doctype, комментарий, `HTML`). То есть это **не** BUG-982 (узлы не попадали в
дерево) и **не** фильтр `SHOW_COMMENT` (закрыт [BUG-326](BUG-326-FIXED.md)) —
узлы на месте, обход их не видит.

## Почему это хуже, чем «метод не работает»

Обёртка развалена наполовину, и работающая половина маскирует сломанную. На том
же document-корневом уокере:

```
w.nextNode()      → null
w.firstChild()    → #doctype(html)[t10]
w.nextSibling()   → #comment( top-comment )[t8]
w.lastChild()     → HTML[t1]
w.previousNode()  → null
```

Идиома `while ((n = w.nextNode())) {…}` — то, чем пользуется реальный код, —
завершается на первой же итерации и читается как «в этом документе нет ни
комментариев, ни элементов». Ошибки нет, исключения нет, `w.root === document`
и `w.currentNode === document` отвечают правдиво. Ровно так и выглядело
наблюдение живой пробы E2E-2 («обход всего документа вернул 0 узлов»),
приписанное было `SHOW_COMMENT`.

## Причина

`document` — объектный литерал (`web_api_shim_mid.js:9422`) и **не носитель
`__nid__`**; это записано прямо в шиме (`:2823`), и для таких обходов там уже
заведён `_lumen_tree_nid(n)` (`:2832`), который отображает `document` в
`_lumen_root_nid`. `TreeWalker`/`NodeIterator` его не используют:

1. `_TreeWalker.prototype._root_nid` (`:5181`) читает `this.root.__nid__` →
   для `document` возвращает `null`.
2. `nextNode` (`:5321`) на этом `null` уходит в `if (root === null) return null`
   до всякого обхода; `previousNode` (`:5304`) выходит по `cur === root`
   (`null === null`).
3. `_NodeIterator.prototype._ensure` (`:5349`) повторяет ту же строку дословно и
   строит пустой `this._all` — второй экземпляр одного дефекта в соседнем коде.
4. `firstChild`/`lastChild` (`:5225`, `:5246`) при этом **работают по случайности**:
   там `_lumen_get_children(this._cur_nid() || 0)`, а `_lumen_root_nid === 0`,
   так что `null || 0` попадает в корень документа. Отсюда и расхождение выше.
   Совпадение, а не намерение: любой сдвиг нумерации арены превратит это в
   тихий обход чужого поддерева.

Побочно, тем же местом: `_nf_accepts` (`:5143`) делит узлы только на
text/comment/**всё остальное = SHOW_ELEMENT**, поэтому `DOCTYPE` (`nodeType 10`,
проба его так и печатает) проходит под битом `SHOW_ELEMENT`. Наблюдаемо это
только на document-корневом уокере (doctype больше нигде не бывает ребёнком),
то есть чинится тем же изменением: DOM §4.5 требует `SHOW_DOCUMENT_TYPE` (0x200),
`SHOW_DOCUMENT_FRAGMENT` (0x400) и `SHOW_DOCUMENT` (0x100) — все три объявлены в
константах (`:5136-5138`) и не используются нигде.

## Масштаб

* Форма `createTreeWalker(document, …)` — канонический пример из DOM §4.5 и то,
  чем пользуются сканеры/санитайзеры/подсветчики на странице. `document.body`
  как корень работает, поэтому дефект виден не всем.
* `dom/traversal/TreeWalker.html` и `NodeIterator*.html` в WPT сейчас
  не доходят до ассертов вовсе — их `setup()` падает на
  [BUG-863](BUG-863-OPEN.md) (`createCDATASection`), так что прогон об этом
  дефекте не свидетельствует и после починки BUG-863 счёт по категории
  изменится ещё раз.
* Цены на реальных страницах не измерялось.

## Что чинить

Точечно, не проектированием:

1. `_TreeWalker.prototype._root_nid`, `_cur_nid` и `_NodeIterator._ensure` —
   через `_lumen_tree_nid(...)` вместо `.__nid__`, чтобы `document` и
   `DocumentFragment` (у него `__nid__` есть — проба это подтверждает) давали
   один и тот же путь;
2. убрать `|| 0` в `firstChild`/`lastChild` — после п.1 он не нужен, а сейчас
   это молчаливый фолбэк в корень арены;
3. возвращать сам `document`, а не `_lumen_make_element(root_nid)`, когда обход
   выходит на корень (`parentNode`), — иначе `w.parentNode() === document`
   ответит `false`;
4. `_nf_accepts` — честный `nodeType` для doctype/фрагмента/документа и биты
   `SHOW_DOCUMENT_TYPE`/`SHOW_DOCUMENT_FRAGMENT`/`SHOW_DOCUMENT`;
5. регресс-тест рядом с `crates/js/src/dom/tests/` на форму
   «`createTreeWalker(document, SHOW_COMMENT)` находит комментарий до `<html>`»
   — ассерт именно на счёт, а не на «не бросило».
