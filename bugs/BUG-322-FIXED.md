# BUG-322: `instanceof Element`/`Node`/`HTML*Element` всегда `false` для нативных элемент-обёрток

**Статус:** FIXED 2026-07-21
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
с ссылкой на этот баг. **Обновлено фиксом** — оба сабтеста теперь `expected: PASS`.

## Фикс

Реализовано ровно то, что описано в «Ожидании» — таблица тег → интерфейс плюс единая точка
простановки `[[Prototype]]` в `_lumen_build_element` (`crates/js/src/dom.rs`):

- `_lumen_html_tag_prototypes` — объект `TAGNAME → HTML*Element`-конструктор для ~40 общеупотребимых
  HTML-тегов (div/span/p/h1-h6/a/input/button/select/option/textarea/label/form/ul/ol/li/table и
  ячейки/секции/script/style/link/meta/html/head/body/title/canvas/video/audio/iframe/template/pre/
  br/hr/dialog/img). Тегов вне таблицы (в т.ч. кастомных элементов, `<footer>`/`<nav>`/`<section>` и
  т.п.) — намеренное упрощение: HTML LS §3.1.3 отдаёт им сам `HTMLElement`, а не
  `HTMLUnknownElement` (последний зарезервирован под по-настоящему нераспознанные имена тегов;
  различать это не пытаемся).
- `_lumen_element_prototype_for(nid)` — резолвит итоговый прототип: не-HTML-namespace узлы (SVG/
  MathML) получают общий `Element.prototype` (SVG-шим, `svg.rs`, донастраивает конкретные
  `SVG*Element.prototype` уже ПОСЛЕ этого через собственный `Object.setPrototypeOf` на результате
  `createElementNS` — цепочка не конфликтует, потому что `class SVGElement extends Element`);
  HTML-namespace узлы смотрят в таблицу выше, фолбэк — `HTMLElement.prototype`.
- В хвосте `_lumen_build_element`, перед `return _obj`, один вызов:
  `Object.setPrototypeOf(_obj, _lumen_is_text_node(nid) ? Text.prototype : _lumen_element_prototype_for(nid))`.
  Текстовые узлы (включая `document.createComment`, которое под капотом строит текстовый узел —
  см. BUG-325, отдельный, не тронутый здесь гэп) получают `Text.prototype`.
- Побочная находка по дороге: `function HTMLImageElement() {}` (BUG-305) не имел вообще никакой
  `.prototype`-цепочки (голый `Object.prototype`) — если бы `<img>`-тег молча смотрел на него из
  новой таблицы, `instanceof Element`/`Node`/`HTMLElement` ломались бы именно для `<img>`, единственного
  тега с отдельной, более богатой обёрткой. Поправлено на тот же паттерн, что и остальные
  `HTML*Element`-интерфейсы: `throw` в конструкторе + `Object.create(HTMLElement.prototype)`.

Регрессионный юнит-тест `element_prototype_chain_instanceof` (`crates/js/src/dom.rs`, рядом с
`character_data_prototype_chain`) проверяет цепочку прототипов и `instanceof` для div/span/body
(теговая таблица), незарегистрированного тега (фолбэк на `HTMLElement`, не на конкретный
подкласс) и текстового узла (`instanceof Text`/`CharacterData`/`Node`, не `Text` для элемента).
Прогнан против V8 (`--features v8-backend`) — весь пакет `lumen-js` зелёный (2506 + 68 тестов).

Риск для `Object.keys`/`JSON.stringify`/`for...in`, о котором предупреждало «Замечание по объёму»,
не материализовался: `Object.keys`/`getOwnPropertyNames`/`JSON.stringify` читают только
СОБСТВЕННЫЕ свойства (не задеты добавлением `[[Prototype]]`), а единственное найденное в
вендоренном корпусе использование `for...in` над самим элементом
(`dom/nodes/attributes.html:719`, `getEnumerableOwnProps1`) явно фильтрует через
`obj.hasOwnProperty(prop)`, так что унаследованное перечисляемое `constructor` с прототипов
(существующий, не новый для этого фикса паттерн — `X.prototype.constructor = X` без
`enumerable: false`, использован по всему `WEB_API_SHIM` для Event-подклассов и раньше) туда не
просачивается. Не переделывал этот паттерн на non-enumerable по всему файлу — предсуществующий,
не завязанный на этот баг, риск не подтверждён тестами.

Кастомные элементы (`_lumen_ce_upgrade_element`) и распарсенные (не через `createElementNS`)
SVG/MathML-узлы намеренно не тронуты — отдельные, не пересекающиеся периметры.
