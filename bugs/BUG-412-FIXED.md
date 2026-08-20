# BUG-412 — `document.getElementsByName()` отсутствует в шиме целиком

**Статус:** FIXED 2026-08-20 (P3, ветка `p3-bug-412-get-elements-by-name`)
**Компонент:** js (`crates/js/src/dom.rs` — блок tree-accessor'ов живого `document`,
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
это уже заведённый [BUG-384](BUG-384-FIXED.md) (проба: `window.n1` === `undefined` при
`<img name=n1>` в документе).

## Что сделано (2026-08-20, P3)

`document.getElementsByName(elementName)` заведён в том же объектном литерале, что
`getElementsByTagName`/`getElementsByClassName`. Три отличия от «направления починки» выше,
каждое — потому что дешёвая инфраструктура для этого в шиме уже была:

1. **Живой `NodeList`, а не статический массив.** Возврат идёт через
   `_lumen_make_nid_collection` — тот же Proxy, на котором уже живёт `document.images`, —
   поэтому коллекция перезапрашивает дерево на каждом обращении и переживает
   вставку/удаление узлов. Статический массив оставлял бы красными два вендоренных файла
   категории (`-liveness.html`, `-interface.html`), которых нет в таблице выше.
2. **Интерфейс `NodeList` (DOM §4.2.10.1) заведён отдельно от `HTMLCollection`.**
   HTML LS §3.1.5 требует именно `NodeList`, а `-interface.html` проверяет обе половины:
   `instanceof NodeList` и **не** `instanceof HTMLCollection`. Прототип — маркерный, как у
   `HTMLCollection` (конструктор бросает `Illegal constructor`), с `forEach`,
   `Symbol.iterator` и `Symbol.toStringTag` из общего блока регистрации;
   `entries`/`keys`/`values` не реализованы. Общий Proxy получил третий параметр
   `noNamed`, снимающий именной доступ (`namedItem`, `list['имя']`, именные own-keys) —
   это поведение `HTMLCollection` (DOM §4.2.10.2), у `NodeList` его нет.
3. **Селектор `[name]` + сравнение значения в JS, а не собранный `[name="…"]`.**
   Так аргумент с кавычками, обратными слэшами или переводом строки не требует
   CSS-экранирования, а фильтр по HTML-пространству имён (`_lumen_is_html_namespace`)
   отсекает `<svg name=x>`/`<math name=x>`, которые селектор атрибута поймал бы
   (`-namespace.html`). Регистр не складывается: `name` не входит в список атрибутов,
   значения которых селекторы сравнивают ASCII-регистронезависимо, — `matches_attribute`
   в `layout/src/style.rs` сравнивает побайтово, чего и требует `-case.html`.

`String(elementName)` на входе даёт WebIDL-преобразование `DOMString`, из-за которого
`getElementsByName(null)` ищет имя `null`, а `getElementsByName(undefined)` — `undefined`
(`-null-undef.html`).

## Проверка

* Юнит-тесты (`crates/js/src/dom.rs`, модуль `v8_core`): `get_elements_by_name_document`
  (совпадение по имени, регистр, `id` не в счёт, чужое пространство имён, `null`/`undefined`)
  и `get_elements_by_name_is_live_node_list` (liveness 1→2→1, `instanceof`, class string,
  отсутствие `namedItem`, `item()`, `forEach`).
* Весь набор крейта на дефолтном движке: `cargo test -p lumen-js --features v8-backend`
  — **2890 passed, 0 failed** (правка общего Proxy не задела `form.elements`,
  `select.options`, `children`, `document.images`).
* Живая проба на собранном `dev-release`-бинаре (`--dump-layout`, страница из ассертов
  всех семи вендоренных `.html`-файлов категории + две проверки на регрессию соседних
  коллекций) — **20 PASS / 1 FAIL**:

  ```
  PASS getElementsByName in document      PASS null -> "null"
  PASS case: ABCD -> 0 / abcd -> 1        PASS undefined -> "undefined"
  PASS id does not match                  PASS interface: instanceof NodeList
  PASS newelements: length + identity     PASS interface: not HTMLCollection
  PASS param: length + identity           PASS interface: class string
  FAIL namespace: only the HTML element   PASS liveness: 1 -> 2 -> 1
  PASS namespace: it is the <p>           PASS children.namedItem still works
                                          PASS document.images is HTMLCollection
  ```

  Единственный FAIL — не этот дефект и не регрессия: `<svg name=x>`, **разобранный из
  разметки**, лежит в шиме в пространстве имён `http://www.w3.org/1999/xhtml`
  (`svg.namespaceURI` печатает именно его, `tagName` — `SVG`), потому что HTML-парсер
  не реализует foreign content (HTML LS §13.2.6.5) вообще — это [BUG-685](BUG-685-OPEN.md).
  Фильтр здесь корректен: тот же `<svg>`, созданный через `createElementNS`, отсекается,
  что и проверяет юнит-тест. `-namespace.html` пройдёт вместе с BUG-685, точки правки в
  этом баге не осталось.

Вне скоупа (заведено отдельно): именованный доступ `window.<name>` — [BUG-384](BUG-384-FIXED.md);
tree-accessor'ы отсоединённого документа — [BUG-415](BUG-415-OPEN.md);
`Element.prototype.getElementsByName` спекой не предусмотрен (метод только на `Document`).

