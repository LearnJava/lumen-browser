# BUG-416 — `Element.prototype.getElementsByTagName` отсутствует, `getElementsByTagNameNS` отсутствует и на элементе, и на документе

**Статус:** FIXED 2026-08-22
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
([BUG-412](BUG-412-FIXED.md)) и этот; tree-accessor'ы в шиме заведены поштучно, а не как набор.

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

## Исправлено 2026-08-22 (P3)

Оба метода заведены на элементе, `…NS` — и на документе; попутно исправлена **третья,
не заявленная в баге половина дефекта**, найденная при чтении кода: документный
`getElementsByTagName` существовал, но искал не то.

**Направление «делегировать в селекторный движок» отвергнуто по измерению.** Старое тело
документного метода отдавало имя тега как CSS type-селектор
(`_lumen_query_selector_all(String(tag))`), а type-селектор сопоставляется **точным
равенством строк** с локальным именем (`style.rs:9602`, `matches_simple`) — CSS-парсер имя
селектора не лоуэркейсит (`parser.rs::parse_ident`). Отсюда два молчаливых расхождения со
спекой, которых в баге не было:

* `document.getElementsByTagName('DIV')` возвращал **пустой список** вместо каждого
  HTML-`<div>` — DOM LS §4.5 требует складывать регистр для элементов HTML-namespace;
* имя, не являющееся идентификатором (`'a b'`, `'1'`), разбиралось как другой селектор или
  ни как какой, то есть промах превращался в чужой ответ.

Поэтому сопоставление имён вынесено из селекторного движка в шим — три общих помощника в
`crates/js/src/dom.rs` (`_lumen_tag_name_predicate`, `_lumen_tag_ns_predicate`,
`_lumen_collect_matching`), а движок используется только как обходчик поддерева в
tree-order (`'*'`). Правило §4.5 реализовано дословно: элемент HTML-namespace сопоставляется
по ASCII-сложенному регистру, любой другой (SVG, MathML, без namespace) — точным
qualified-именем; у `…NS` `null` и `''` одинаково значат «без namespace», `'*'` работает в
обеих позициях, локальное имя сравнивается регистро-зависимо. Признак элемента — `null`
локального имени у всякого неэлементного узла, поэтому `getElementsByTagName('*')` больше
не может зацепить текстовый узел.

Потребителей три, все переведены на общие предикаты: живой документ, живая обёртка элемента
(`_lumen_build_element`, поиск ограничен потомками — тот же класс, что чинил
[BUG-298](BUG-298-FIXED.md) для `querySelectorAll`) и отсоединённый документ
([BUG-415](BUG-415-FIXED.md), 2026-08-22) — у последнего своя реализация
`getElementsByTagName` появилась в тот же день; она сопоставляла имена уже правильно, но
её ветка `'*'` возвращала **и текстовые узлы**, потому что `_detached_walk` обходит всех
детей, а не только элементы. Теперь и там общий предикат, и заодно добавлен
`getElementsByTagNameNS`.

**Проверка.** 5 юнит-тестов в `dom.rs` (скоуп элемента, складывание регистра для HTML,
регистро-зависимость foreign content, `'*'` только по элементам, `…NS` на документе и на
элементе) + живая проба `--dump-layout` на dev-release, воспроизводящая таблицу из «Симптома»:
`el.getElementsByTagName=function`, `el.getElementsByTagNameNS=function`,
`doc.getElementsByTagNameNS=function`, `el.getElementsByTagName('P').length=2`,
`document.getElementsByTagName('DIV').length=1`. `cargo test -p lumen-js --features v8-backend` —
2915 + 70 зелёных, регрессий нет.

## Что осталось за границей фикса (оба — чужие открытые баги)

* **Два WPT-файла из «Данных WPT» этим фиксом не флипнутся.**
  `Element.getElementsByTagName-foreign-0*.html` проверяют регистро-зависимость на
  foreign content, **разобранном из разметки**, а парсер не реализует HTML LS §13.2.6.5:
  инлайновый `<svg><linearGradient>` приходит с `namespaceURI` HTML и локальным именем
  `lineargradient` — измерено этой же пробой (`parsed grad ns=…/1999/xhtml tag=LINEARGRADIENT`,
  при том что `createElementNS(SVG,'linearGradient')` даёт правильные `…/2000/svg` и
  `linearGradient`). Это [BUG-685](BUG-685-OPEN.md), не остаток этого бага: предикат
  различает namespace верно, ему подают неверный namespace. Смежное сужение того же
  `enum Namespace` со стороны `createElementNS` — [BUG-830](BUG-830-OPEN.md).
* **`'getElementsByTagName' in Element.prototype` по-прежнему `false`** — строка из
  «Симптома», которая НЕ является признаком отсутствия метода: у этой фабрики обёрток все
  ~120 членов интерфейса лежат собственными свойствами инстанса, а не операциями прототипа,
  так что `'querySelectorAll' in Element.prototype` тоже `false` (проверено той же пробой).
  Это [BUG-747](BUG-747-OPEN.md) (переработка фабрики), общий для всех членов.
