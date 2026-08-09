# BUG-715 — `DOMTokenList`/`CSSStyleDeclaration` have no global constructor and no indexed-property WebIDL shape

**Статус:** OPEN
**Компонент:** js (`crates/js/src/dom.rs:1327-1381` — `_lumen_make_class_list`; `dom.rs:1404+` — `_lumen_make_style`, `CSSStyleDeclaration`)
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
