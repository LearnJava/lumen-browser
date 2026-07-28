# BUG-412 — `document.getElementsByName()` отсутствует в шиме целиком

**Статус:** OPEN
**Компонент:** js (`crates/js/src/dom.rs:7048-7062` — блок tree-accessor'ов живого `document`,
рядом с уже реализованными `getElementsByTagName`/`getElementsByClassName`)
**Найден:** 2026-07-28 (P2), WPT-VENDOR-html, срез `html/dom`

## Симптом

```
document.getElementsByName is not a function
```

Проба (`--dump-layout`, обычная страница):

```
doc.getElementsByName=undefined | in-doc=false | doc.getElementsByTagName=function
```

`'getElementsByName' in document` === `false` — метод не определён ни на живом `document`,
ни где-либо ещё; соседние аксессоры того же раздела спеки (`getElementsByTagName`,
`getElementsByTagNameNS`, `getElementsByClassName`, `getElementById`) присутствуют.

## Что требует спека

HTML LS §3.1.5 «DOM tree accessors»: `document.getElementsByName(elementName)` возвращает
**живой** `NodeList` всех элементов документа, у которых атрибут `name` равен аргументу
(сравнение по строке, чувствительное к регистру; аргумент приводится через `DOMString`,
`null` → строка `"null"`).

## Данные WPT

Срез `html/dom` (`run_report.py --all --root html/dom --recursive`), 4 файла категории
`documents/dom-tree-accessors/document.getElementsByName/`, 31 FAIL-сабтест, все с одним
и тем же сообщением:

| Файл | FAIL |
|---|---|
| `document.getElementsByName-newelements.html` | 27 |
| `document.getElementsByName-null-undef.html` | 2 |
| `document.getElementsByName-case.html` | 1 |
| `document.getElementsByName-id.html` | 1 |

## Направление починки

В том же объектном литерале, что `getElementsByClassName` (`dom.rs:7058-7062`), — через
существующий `_lumen_query_selector_all` с селектором `[name="…"]` (экранировать кавычки в
аргументе) и `.map(_lumen_make_element)`. Это даст статический массив, как у соседей;
живость `NodeList` — то же осознанное упрощение, что уже задокументировано для
`getElementsByTagName`/`getElementsByClassName` («Static array, not a live HTMLCollection»),
и отдельного бага не требует.

Смежно (в этот баг не входит): именованный доступ через `window.<name>` тоже отсутствует —
это уже заведённый [BUG-384](BUG-384-OPEN.md) (проба: `window.n1` === `undefined` при
`<img name=n1>` в документе).
