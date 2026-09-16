# BUG-891 — XSLT и XPath отсутствуют целиком: `XSLTProcessor`, `document.evaluate`, `XPathEvaluator`/`XPathResult` не определены (`DOMParser`/`XMLSerializer` при этом работают)

**Статус:** ЗАКРЫТ 2026-09-16 (ветка `p1-gap-xpath`, [GAP-XPATH](../ROADMAP.md)) — XPath-часть реализована; XSLT явно выведена из скоупа как отдельное, намного более крупное решение (см. «Что дальше»), не остаётся открытым пунктом.
**Тип:** нереализованная функциональность, не дефект реализованного кода — велась как задача `GAP-XPATH` в [ROADMAP.md](../ROADMAP.md), P3 как баг не брал. Переклассифицировано 2026-09-02 ре-триажем пула WPT-RUN-5/6: срезы заводили багом всё подряд, потому что правила заведения ([docs/probe-method.md §8](../docs/probe-method.md)) тогда ещё не было. Файл сохраняет номер и путь — на него ссылаются CLAUDE.md, STATUS-файлы и python-тулинг, а запись наблюдений остаётся полезной там, где лежит.
**Заведён:** 2026-08-23 (WPT-RUN-6, срез 29 — живой замер, вариант `xslt-xml`)
**Область:** js (`crates/js/src/xpath.rs`)
**Владелец:** P1/P3. Заведён P2 в ходе WPT-задачи, XPath-часть закрыта P1.

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

**Решение о скоупе (2026-09-16, ревизия роадмапа при взятии GAP-XPATH):**
`document.evaluate`/`XPathEvaluator`/`XPathExpression`/`XPathResult`/`XPathException`
реализованы целиком — `crates/js/src/xpath.rs`, полный XPath 1.0 (токенайзер,
рекурсивный спуск, все оси, стандартная функциональная библиотека), навешан на
`Document.prototype` после установки core DOM shim. `XSLTProcessor` остаётся
явно ВНЕ СКОУПА: отдельный, намного более крупный трансформ-движок — тот же
класс решения, что и у [GAP-SMIL](../ROADMAP.md) ("объявить вне скоупа").
Категория `xml/xslt` (21 файл WPT) остаётся known-failing без отдельной
задачи; `domxpath/` (14 файлов) закрывается этим фиксом за вычетом
`xpathevaluatorbase-creatensresolver.html`, если тот упирается в путь
`DOMParser().parseFromString()` — см. следующий абзац.

Известный остаточный разрыв внутри реализованного XPath: `document.evaluate`
патчится на `Document.prototype` (покрывает живой документ страницы и
`document.implementation.createDocument`/`createHTMLDocument`), но
`DOMParser().parseFromString()` строит отдельный closure-private `VDocument`
(`crate::dom_parser`), которого этот файл снаружи не видит — XPath на
распарсенном из строки документе не подключён. Оставлено на будущий срез,
тот же инкрементальный стиль, что и остальной роадмап.
