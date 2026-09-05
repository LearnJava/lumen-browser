# BUG-715 — `DOMTokenList`/`CSSStyleDeclaration` have no global constructor and no indexed-property WebIDL shape

**Статус:** FIXED 2026-09-05 (P3)
**Компонент:** js (`crates/js/src/shim/web_api_shim_mid.js` — `_lumen_make_class_list`/`_lumen_make_attr_token_list`; `_lumen_make_style`, `CSSStyleDeclaration`)
**Найден:** P2, WPT-VENDOR-webidl, 2026-08-09

## Симптом

Тот же прогон категории `webidl`, что и [BUG-714](BUG-714-OPEN.md)
(45 id, 37/45 harness OK, 134/324 сабтестов). Второй по величине кластер —
все четыре файла `ecmascript-binding/legacy-platform-object/*.html`
(`DefineOwnProperty`/`GetOwnProperty`/`OwnPropertyKeys`/`Set`, 0/4, 0/3, 0/5,
3/11 — **20 unexpected из 23**) плюс три сабтеста
`ecmascript-binding/put-forwards.html` (`CSSStyleDeclaration is not defined`
/ `DOMTokenList is not defined`, ×3).

Тестовый фикстур `legacy-platform-object/DefineOwnProperty.html` берёт
`span.classList` (`DOMTokenList`) и проверяет WebIDL §3.9 "legacy platform
object" контракт для интерфейсов, поддерживающих indexed/named свойства:
`[[GetOwnProperty]]`/`[[DefineOwnProperty]]`/`[[OwnPropertyKeys]]` должны
представлять `domTokenList[0]` как настоящее (non-configurable/writable
через explicit exotic behavior) собственное свойство объекта — то есть
`Object.getOwnPropertyDescriptor(domTokenList, "0")` обязан вернуть
дескриптор. Живая проверка кода: `_lumen_make_class_list`
(`dom.rs:1327-1381`) строит `classList` как обычный object-literal с
методами `contains`/`add`/`remove`/`toggle`/`replace`/`item`/`forEach`/
`toString` и геттером `length` — **никакого `[i]`-доступа, никакого
`Symbol.iterator`, никакого global `DOMTokenList`-конструктора вообще**.
`domTokenList[0]` просто не существует (`undefined`), поэтому:

- `assert_prop_desc_equals(domTokenList, "0", {...})` падает на
  `Object.getOwnPropertyDescriptor(...)` → `undefined`, дальнейшее
  `.hasOwnProperty` бросает `TypeError` (`DefineOwnProperty.html`,
  `GetOwnProperty.html`, `OwnPropertyKeys.html` — все три файла).
- `Reflect.ownKeys(domTokenList)` бросает `TypeError: Reflect.ownKeys
  called on non-object` в двух сабтестах `OwnPropertyKeys.html` — по
  тексту трассы похоже на артефакт способа, которым сам объект
  сконструирован (не настоящий exotic object), не за пределы данного
  файла.
- `put-forwards.html`: `new DOMTokenList()`/`DOMTokenList.prototype` в
  тестовом коде обращается к глобальному классу `DOMTokenList` напрямую
  (`a.relList instanceof DOMTokenList` — тестовый паттерн, где `relList`
  сам работает, но тип нельзя проверить) → `ReferenceError: DOMTokenList
  is not defined`, потому что `globalThis.DOMTokenList` не установлен
  вовсе, только фабрика-функция `_lumen_make_class_list` возвращает
  инстансы без какого-либо связанного конструктора/прототипа.

`CSSStyleDeclaration` (`_lumen_make_style`, `dom.rs:1404+`) — тот же
паттерн: `Proxy`-based object без global `CSSStyleDeclaration` конструктора
(`put-forwards.html`: `element.style instanceof CSSStyleDeclaration` →
`ReferenceError: CSSStyleDeclaration is not defined`, 2 сабтеста).

`legacy-platform-object/Set.html` (3/11 passed) — частичный сигнал,
вероятно тот же корень для другого фикстура (`HTMLSelectElement`
indexed setter, out of scope этого разбора — не проверялось детально,
см. «Дальше»).

## Масштаб

Общий архитектурный паттерн: DOM-интерфейсы, которые WebIDL описывает как
"legacy platform object" с indexed/named property support
(`DOMTokenList`, `CSSStyleDeclaration`, вероятно `HTMLCollection`/
`NamedNodeMap` — не проверялись в этой сессии), в Lumen реализованы как
ad-hoc фабричные функции (`_lumen_make_*`), возвращающие обычные `{}`-объекты
с явными методами вместо настоящих `class X { ... }` с
`constructor`/`prototype`/`[[DefineOwnProperty]]`-подобным поведением.
Отличается от уже известного класса «конструктор существует, но не
guard-ится» ([BUG-711](BUG-711-OPEN.md)/[BUG-712](BUG-712-OPEN.md)/
[BUG-713](BUG-713-OPEN.md)/[BUG-672](BUG-672-OPEN.md)) — здесь глобального
конструктора нет вообще, а не отсутствует guard на существующем.

## Причина

`_lumen_make_class_list` (`dom.rs:1327`) и `_lumen_make_style`
(`dom.rs:1404`) — фабричные функции, инстанс каждая создаёт как
объектный литерал (`var cl = {...}`), не через `new SomeClass()`. Ни один
из них не выставляет глобальную функцию-конструктор
(`globalThis.DOMTokenList = ...`/`globalThis.CSSStyleDeclaration = ...`),
и ни один не реализует индексный доступ (`[i]`) — только явный метод
`.item(i)` у `DOMTokenList`. Не установлена точная причина отсутствия
`[i]`-доступа целиком (вне скоупа WPT-VENDOR-задачи) — вероятно, тот же
класс упрощения, что у остальных `_lumen_make_*`-фабрик: WebIDL
"legacy platform object" trap-семантика (indexed getter/setter через
`Proxy` или явные `Object.defineProperty` в цикле по `length`) нигде
в шиме не реализована как переиспользуемый хелпер.

## Дальше

Fix scope (для P3): (1) добавить глобальные `DOMTokenList`/
`CSSStyleDeclaration` конструкторы с настоящим `.prototype`, на который
переносятся сейчас-инстансные методы; (2) добавить indexed-property
support через `Proxy` (get trap на числовые ключи → `getArr()[i]`,
defineProperty/set trap → `TypeError` для read-only индексов) — вероятно
стоит сделать переиспользуемым хелпером
(`_lumen_make_indexed_readonly_proxy(getArr)`), раз минимум два интерфейса
нуждаются в одном и том же паттерне; (3) `Set.html` (3/11) — разобрать
отдельно, возможно другой корень (`HTMLSelectElement` indexed setter).
Не требует новой инфраструктуры для воспроизведения — все четыре файла
исполняются локально без TLS/testdriver-гэпа.

## Исправление (2026-09-05, P3)

Пункты (1) и (2) из scope реализованы в `crates/js/src/shim/web_api_shim_mid.js`:

- **`DOMTokenList`** — реальный `function DOMTokenList() { throw new
  TypeError('Illegal constructor'); }` с методами
  (`contains`/`add`/`remove`/`toggle`/`replace`/`item`/`forEach`/`toString`)
  и аксессорами (`length`, `value`, `Symbol.toStringTag`) на
  `DOMTokenList.prototype` — тот же приём, что `ShadowRoot`/`HTMLCollection`/
  `NodeList` уже используют (общий конструктор без `new`, методы на общем
  прототипе, а не пересоздаются на каждый инстанс). Инстанс несёт только
  `__nid__`/`__attrName__`; `_lumen_make_attr_token_list`/
  `_lumen_make_class_list`/`_lumen_make_rel_list` (`relList`, BUG-826)
  используют тот же фабричный путь без изменений в вызывающем коде.
- **`CSSStyleDeclaration`** — та же схема: `getPropertyValue`/`setProperty`/
  `removeProperty`/`cssText` перенесены на `CSSStyleDeclaration.prototype`
  (были инстансными замыканиями поверх `handler`-литерала), инстанс несёт
  только `__nid__`. Это одновременно чинит `put-forwards.html`'s паттерн
  `Object.getOwnPropertyDescriptor(CSSStyleDeclaration.prototype, "cssText")`
  — раньше `cssText` был собственным свойством каждого инстанса, а не
  прототипа.
- **Indexed-property Proxy** — новый переиспользуемый хелпер
  `_lumen_make_indexed_readonly_proxy(target, getArr)` (ровно то, что
  предлагал этот баг): `get`/`has`/`ownKeys`/`getOwnPropertyDescriptor`
  представляют `list[i]` как настоящее own-property (WebIDL §3.9
  `[[GetOwnProperty]]`/`[[OwnPropertyKeys]]`), `defineProperty`/`set`
  возвращают `false` для ЛЮБОГО array-index ключа (в диапазоне или нет) —
  это даёт `Object.defineProperty(list, "0", …)` → `TypeError` и
  `list[0] = x` → тихий no-op в sloppy/`TypeError` в strict через штатный
  движковый `PutValue`, не через собственный `throw`. Используется в
  `_lumen_make_attr_token_list` для `DOMTokenList`.
- Регресс-тесты: `crates/js/src/dom/tests/v8_events_cache.rs`
  (`classlist_instanceof_dom_token_list`, `classlist_indexed_access`,
  `classlist_define_own_property_on_index_throws`,
  `classlist_object_keys_sees_indices`,
  `style_instanceof_css_style_declaration`,
  `style_css_text_is_on_shared_prototype`). Логика Proxy-трапов
  дополнительно прогнана вне движка — стендовый Node-харнесс с реальными
  ассертами из `DefineOwnProperty.html`/`GetOwnProperty.html` (вырезан
  перед коммитом, не часть репозитория) подтвердил побитовое совпадение с
  ожиданиями теста, включая strict/sloppy-режимы `[[Set]]`.

**Остаток — вне scope этого бага, отдельно от DOMTokenList/CSSStyleDeclaration.**
Чтение реальных вендоренных файлов (`tests/wpt/webidl/ecmascript-binding/
legacy-platform-object/{DefineOwnProperty,GetOwnProperty,OwnPropertyKeys,Set}.html`)
показало, что пункт (3) из scope был неточным: `Set.html` вообще не
упоминает `DOMTokenList`/`CSSStyleDeclaration` — все 11 его сабтестов о
`childNodes`(`NodeList`)/`attributes`(`NamedNodeMap`)/`form.method`/
`sessionStorage`. Аналогично `DefineOwnProperty.html` (0/4 на момент
заявки) содержит ещё три сабтеста про `HTMLSelectElement` (indexed
setter), `HTMLCollection` (`dataList.options`, named getter без setter,
`LegacyUnenumerableNamedProperties`) и `DOMStringMap`(`dataset`, named
setter) — независимые от этого бага дефекты/пробелы, не проверялись живым
прогоном движка в рамках этой правки. Следующему триажу этой категории
читать эти файлы как источник, не как «то же самое, что BUG-715».
