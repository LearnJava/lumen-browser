# BUG-416 — `Element.prototype.getElementsByTagName` отсутствует, `getElementsByTagNameNS` отсутствует и на элементе, и на документе

**Статус:** OPEN
**Компонент:** js (`crates/js/src/dom.rs:5516-5880` — фабрика живых обёрток `_lumen_build_element`;
документный аналог — `dom.rs:7045-7062`)
**Найден:** 2026-07-28 (P2), WPT-VENDOR-html, срез `html/syntax`

## Симптом

Проба (`--dump-layout`, `<div id=w><p class=c>x</p><p class=c>y</p></div>`):

```
el.getElementsByTagName=undefined    el.getElementsByTagNameNS=undefined
el.getElementsByClassName=function   el.querySelectorAll=function
proto.getElementsByTagName=false     (то есть нет и на Element.prototype)
doc.getElementsByTagName=function    doc.getElementsByTagNameNS=undefined
```

Разрыв неравномерный: у элемента есть `getElementsByClassName` и `querySelectorAll`, но нет
`getElementsByTagName`; у документа — наоборот, `getElementsByTagName` есть, а
`getElementsByTagNameNS` нет ни там, ни там.

## Это незакрытый остаток уже закрытого бага

[BUG-279](BUG-279-FIXED.md) (FIXED 2026-07-13) чинил `document.getElementsByTagName` и в своей
же строке зафиксировал: «`Element.prototype.getElementsByTagName` и
`document.getElementsByClassName` всё ещё отсутствуют (не в этом фиксе)». Вторую половину
позже закрыл [BUG-302](BUG-302-FIXED.md) (`Element.getElementsByClassName`), первая осталась
незаведённой — то есть остаток был **записан в закрытом баге, но не перенесён в открытую
строку**, и пролежал 15 дней невидимым. `getElementsByTagNameNS` в том остатке не упоминался
вовсе.

## Что требует спека

DOM LS §4.9 «Interface `ParentNode`» / §4.5–4.6: `getElementsByTagName(qualifiedName)` и
`getElementsByTagNameNS(namespace, localName)` определены **и** на `Document`, **и** на
`Element` (у элемента поиск ограничен его поддеревом — ровно тот класс ошибки, что
[BUG-298](BUG-298-FIXED.md) чинил для `querySelectorAll`). Специальные значения:
`getElementsByTagName('*')` — все элементы; в HTML-документе имя сопоставляется
без учёта регистра для HTML-namespace и с учётом — для остальных; у `…NS` аргумент
`namespace` `'*'` означает любой, пустая строка — `null`.

## Данные WPT

Срез `html/syntax` (`run_report.py --all --root html/syntax --recursive`):

| Файл | Сообщение |
|---|---|
| `parsing/Element.getElementsByTagName-foreign-01.html` | `wrapper.getElementsByTagName is not a function` (2) |
| `parsing/Element.getElementsByTagName-foreign-02.html` | `wrapper.getElementsByTagName is not a function` (2) |

Оба файла — именно про регистро-зависимость поиска в foreign content (SVG/MathML), то есть про
**вторую** половину дефекта (сопоставление имён по namespace), до которой тест не доходит,
потому что нет самого метода. Отдельно отметим: обе категории (`html/dom` и `html/syntax`)
дали по своему представителю этого семейства — `document.getElementsByName`
([BUG-412](BUG-412-OPEN.md)) и этот; tree-accessor'ы в шиме заведены поштучно, а не как набор.

## Направление починки

Тот же приём, что у уже работающих соседей: делегировать в `_lumen_query_selector_all`,
но с областью поиска от узла (после [BUG-298](BUG-298-FIXED.md) такой вариант в шиме уже
есть — им пользуется `Element.querySelectorAll`), и добавить `…NS`-вариант с фильтрацией по
`namespaceURI`. Живость возвращаемого `HTMLCollection` — то же осознанное упрощение
(«Static array, not a live HTMLCollection»), что уже задокументировано у соседей, отдельного
бага не требует.

Стоит заодно пройти по списку DOM LS §4.5/§4.9 целиком и завести недостающие одним срезом,
а не по одному на категорию — иначе каждая следующая вендоренная категория будет приносить
по такому же баг-номеру.
