# BUG-322: `instanceof Element`/`Node`/`HTML*Element` всегда `false` для нативных элемент-обёрток

**Статус:** OPEN
**Дата:** 2026-07-20
**Компонент:** js (WEB_API_SHIM, `crates/js/src/dom.rs`)
**Найден:** WPT `dom/nodes/Element-children.html` (P2-wpt curated subset), при разборе двух
unexpected FAIL после регенерации `.ini`-храповика 2026-07-20.
**Родитель:** отложенный пункт 3 из [BUG-321](BUG-321-FIXED.md) (там же ошибочно сослались на
[BUG-305](BUG-305-FIXED.md) — это другой, не относящийся к делу, уже закрытый баг про
конструктор `Image`). Этот файл — правильный трекер для того долга; ссылки на `BUG-305` в
`BUG-314-FIXED.md`/`BUG-321-FIXED.md`/`BUGS.md` переставлены сюда в том же коммите.

## Симптом

Любая нативная обёртка элемента (`_lumen_make_element`/`_lumen_build_element` в
`WEB_API_SHIM`) — plain JS-объект без выставленного `[[Prototype]]`. Поэтому:

```js
document.body instanceof Element        // false — должно быть true
document.createElement('div') instanceof HTMLDivElement  // false
document.createElement('div') instanceof HTMLElement     // false
document.createElement('div') instanceof Node            // false
```

Подтверждено юнит-тестом (V8-бэкенд, `cargo test -p lumen-js --features v8-backend`):
`document.body instanceof Element`, `container instanceof Element`,
`container.children.item(0) instanceof Element` — все `false`, хотя объект — настоящая,
полнофункциональная обёртка элемента (`__nid__`, все методы/геттеры на месте).

`BUG-314`/`BUG-321` уже выставили node-family интерфейсы (`Node`/`Element`/`HTMLElement`/
`HTML*Element`) как глобали и завели им прототипы — но ни один существующий элемент-конструктор
(`document.createElement`, парсинг HTML, `_lumen_build_element`) не проставляет
`Object.setPrototypeOf(wrapper, <соответствующий *Element>.prototype)` на созданной обёртке.

## Воспроизведение

```bash
cargo test -p lumen-js --features v8-backend -- --nocapture
# добавить временный eval: `document.body instanceof Element` → false
```

Или WPT: `dom/nodes/Element-children.html`, сабтест «HTMLCollection edge cases» —
`container.children.item("foo")` возвращает настоящий `<img>`-элемент (index 0 после
`ToUint32("foo")` = 0 коэрсии), но `assert_true(result instanceof Element)` падает, потому что
`instanceof Element` ложно для ЛЮБОГО элемента, не только через `HTMLCollection`.

## Ожидание

`_lumen_build_element(nid)` выставляет прототип обёртки по тегу через цепочку
`HTML<Tag>Element.prototype → HTMLElement.prototype → Element.prototype → Node.prototype →
EventTarget.prototype` (DOM Standard §4.9 / HTML Standard §3.1.3), так что
`el instanceof Element`/`instanceof Node`/`instanceof HTMLDivElement` и т.д. работают для
обычных, живых элементов. Аналогично для detached-конструкторов (`new Comment()` и т.п. это уже
делают, см. BUG-314) и для text/comment-узлов (`instanceof CharacterData`/`Text`/`Comment`).

## Замечание по объёму (почему не в этом коммите)

Как и зафиксировано в `BUG-321` при первом обнаружении: это общий долг всей системы
элемент-обёрток, а не локальный фикс — нужно решить, как сопоставлять тег → конструктор
(таблица `HTML<Tag>Element`, аналогичная существующему генерируемому набору интерфейс-глобалов
из BUG-314), где кэшировать прототип на nid, и не сломать ли `Object.keys`/`JSON.stringify`
поведение объекта при переходе с plain-object на объект с non-null `[[Prototype]]` (текущий код
местами полагается на `for...in`/`Object.getOwnPropertyNames` над самим враппером — см. также
[BUG-323](BUG-323-FIXED.md), где та же плоская модель мешала `HTMLCollection`-enumeration (fixed
2026-07-21 — `ownKeys`/`getOwnPropertyDescriptor` traps на `Proxy`, независимо от `[[Prototype]]`).
Риск и объём — за пределами точечного P3-багфикса; годится под отдельную задачу P1/P4-масштаба
(вся система элемент-обёрток), не под "один баг за раз".

## `.ini`

`tests/wpt/metadata/dom/nodes/Element-children.html.ini` — сабтест «HTMLCollection edge cases»
(было ошибочно закреплено под именем `Element-children`, см. BUG-323) закреплён `expected: FAIL`
с ссылкой на этот баг.
