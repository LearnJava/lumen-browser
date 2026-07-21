# BUG-323: живой `HTMLCollection` не поддерживает `for-in`/`Object.getOwnPropertyNames` enumeration

**Статус:** FIXED 2026-07-21
**Дата:** 2026-07-20
**Компонент:** js (WEB_API_SHIM, `crates/js/src/dom.rs`, `_lumen_make_html_collection`)
**Найден:** WPT `dom/nodes/Element-children.html` (P2-wpt curated subset), при разборе
unexpected FAIL после регенерации `.ini`-храповика 2026-07-20.
**Родитель:** отложено ещё в [BUG-310](BUG-310-FIXED.md) ("два сабтеста требуют полной
enumeration-семантики HTMLCollection... вне скоупа BUG-310"), но `.ini` закрепил ожидание под
неверными именами сабтестов (`Element-children`/`Element-children 1` вместо реальных,
title-производных `HTMLCollection edge cases`/`HTMLCollection edge cases 1`), из-за чего гейт
`run_suite.py` красный (unexpected), а не "соответствует ожиданию" — исправлено тем же коммитом.

## Симптом

`_lumen_make_html_collection` (`dom.rs`, ~5094–5130) — `Proxy` с `get`/`has` traps, но без
`ownKeys`/`getOwnPropertyDescriptor`. Поэтому:

```js
var list = container.children;   // живой HTMLCollection, 6 element-детей
for (var p in list) { ... }      // ноль итераций — должно быть '0'..'5' плюс именованные
Object.getOwnPropertyNames(list) // [] — должно включать индексы + видимые id/name-ключи
list.hasOwnProperty('foo')       // false — должно быть true, если 'foo' — видимое имя
```

Подтверждено юнит-тестом (`for (var p in container.children) r.push(p)` → пустой массив,
воспроизводится и на rquickjs, и на V8-бэкенде — код общий, `WEB_API_SHIM`).

## Ожидание

DOM Standard §4.2.10.2 (Named property visibility algorithm) + WebIDL "legacy platform object"
own-property semantics: `HTMLCollection`'s own enumerable keys — числовые индексы `0..length-1`
плюс видимые именованные ключи (id у любого элемента коллекции, либо `name`-атрибут только у
элементов в HTML namespace — см. также [BUG-322](BUG-322-FIXED.md), не путать: namespace-сворачивание
в HTML — отдельный, уже задокументированный предел `createElementNS("", ...)`, не блокирует эту
конкретную семантику). Нужны `ownKeys` (возвращает индексы + видимые имена) и
`getOwnPropertyDescriptor` (enumerable+configurable дескриптор для каждого) traps на `Proxy` в
`_lumen_make_html_collection`.

## Замечание

`Element-children.html`'s второй сабтест дополнительно проверяет
`list[exposedName] instanceof Element` (строка 47) — эта часть зависит от [BUG-322](BUG-322-FIXED.md)
(`instanceof` для нативных обёрток) и не закроется одним только фиксом enumeration-traps. Оба
сабтеста `Element-children.html` остаются `expected: FAIL` до закрытия обоих багов.

## Фикс (2026-07-21)

Добавлены `ownKeys`/`getOwnPropertyDescriptor` traps на `Proxy` в `_lumen_make_html_collection`
(`crates/js/src/dom.rs`), плюс новый хелпер `_lumen_html_collection_own_names(ids)` — строит
список видимых имён (`id`, затем `name`, в порядке дерева, без дублей), тем же id-затем-name
проходом, что уже использует `_lumen_html_collection_named` (`get`/`has`/`namedItem`) — чтобы все
traps коллекции были согласованы друг с другом. `ownKeys` возвращает числовые индексы `0..length-1`
плюс эти имена; `getOwnPropertyDescriptor` — `enumerable: true` для индексов (видны в `for-in`) и
`enumerable: false` для именованных ключей (не видны в `for-in`, но видны в
`Object.getOwnPropertyNames`/`hasOwnProperty`, как в реальных браузерах). Оба конфигурируемы
(`configurable: true`) — `target` (`Object.create(HTMLCollection.prototype)`) расширяем и не имеет
собственных свойств, так что виртуальные дескрипторы не нарушают инварианты Proxy.

Юнит-тест `html_collection_supports_enumeration` (`crates/js/src/dom.rs`) проверяет все три
симптома из "Симптом" выше на простом дереве без edge-case'ов namespace-сворачивания.

Не тронуто (осознанно, вне скоупа): namespace-проверка для `name`-экспозиции (спек требует
"только элементы в HTML namespace") **не** добавлена ни в `ownKeys`, ни в `_lumen_html_collection_named` —
Lumen на данный момент сворачивает `createElementNS("", ...)` в `Namespace::Html` (см. `dom.rs`,
`_lumen_create_element_ns`), так что namespace-проверка всё равно не отличила бы `createElementNS("", "img")`
от настоящего HTML-элемента; добавлять её сейчас было бы косметикой без реального эффекта. Из-за
этого `Element-children.html`'s второй сабтест по-прежнему получает лишний `qux`-ключ и другой
порядок против ожидаемого спеком массива — не блокирует: тест остаётся `expected: FAIL` в любом
случае из-за [BUG-322](BUG-322-FIXED.md) (строка 47, `instanceof`).
