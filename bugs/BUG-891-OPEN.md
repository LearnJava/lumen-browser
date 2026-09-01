# BUG-891 — XSLT и XPath отсутствуют целиком: `XSLTProcessor`, `document.evaluate`, `XPathEvaluator`/`XPathResult` не определены (`DOMParser`/`XMLSerializer` при этом работают)

**Статус:** OPEN (ДОРАБОТКА → [GAP-XPATH](../ROADMAP.md))
**Тип:** нереализованная функциональность, не дефект реализованного кода — ведётся как задача `GAP-XPATH` в [ROADMAP.md](../ROADMAP.md), P3 как баг не берёт. Переклассифицировано 2026-09-02 ре-триажем пула WPT-RUN-5/6: срезы заводили багом всё подряд, потому что правила заведения ([docs/probe-method.md §8](../docs/probe-method.md)) тогда ещё не было. Файл сохраняет номер и путь — на него ссылаются CLAUDE.md, STATUS-файлы и python-тулинг, а запись наблюдений остаётся полезной там, где лежит.
**Заведён:** 2026-08-23 (WPT-RUN-6, срез 29 — живой замер, вариант `xslt-xml`)
**Область:** js (`grep -rn "XSLTProcessor\|XPathEvaluator\|document.evaluate" crates/` — ноль совпадений во всём воркспейсе)
**Владелец:** P1/P3. Заведён P2 в ходе WPT-задачи, здесь не чинится.

## Симптом

`new XSLTProcessor()` — `ReferenceError: XSLTProcessor is not defined`;
`document.evaluate` — `undefined`, `XPathEvaluator`/`XPathResult` — тоже.
Соседние XML-точки исправны и это важно для диагностики: `DOMParser`
разбирает `text/xml` (`documentElement.nodeName` = `root`, потомки на месте),
`XMLSerializer.serializeToString` возвращает корректную строку,
`document.implementation.createDocument` даёт объект. То есть XML-сторона
живая, отсутствует ровно преобразование и адресация.

## Прямое измерение

`tests/wpt/verify_cssom_svg_interface_gaps.py --variant xslt-xml`
(2026-08-23, dev-release, Linux):

```
globals = DOMParser,XMLSerializer
new-XSLTProcessor THREW XSLTProcessor is not defined
DOMParser-xml = root      xml-child = kid
XMLSerializer = <root xmlns="urn:x"><kid
implementation = object   evaluate = undefined
```

## Цена по WPT

4 id снимка WPT-RUN-5: три с текстом `XSLTProcessor is not defined`
(`xml/xslt/document-element.window.html`, `document-function.window.html`,
`transformToFragment.tentative.window.html`) и один с `XPathNSResolver`
(`domxpath/xpathevaluatorbase-creatensresolver.html`); вся категория
`xml/xslt` (21 файл) и `domxpath/` (14 файлов) стоят за этими же глобалами —
`domxpath` дополнительно упирается в [BUG-780](BUG-780-FIXED.md)-подобный
путь загрузки своего раннера.

## Что дальше

Обе части — крупные подсистемы, а не строчки в шиме, и обе внефазовые:
решение по ним (реализовывать, стабить или объявить вне скоупа Phase 3)
принимать вместе с `docs/plan/`, а не в рамках WPT-задачи. Ценность этой
записи — измеренная цена и то, что путь `DOMParser` рядом исправен.
