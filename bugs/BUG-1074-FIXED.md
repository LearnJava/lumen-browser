# BUG-1074 — `Element.nodeValue`/`DocumentFragment.nodeValue` возвращают `undefined` вместо `null`

**Статус:** FIXED 2026-11-15
**Заведён:** 2026-09-21 (P3, побочно при фиксе BUG-1054, `dom/nodes/Node-nodeValue.html`)
**Область:** js (`crates/js/src/shim/web_api_shim_mid.js` — элементная обёртка/`_lumen_build_element`
и `DocumentFragment`; `DocumentType`/`Document` уже отдают `null` верно, строки 3926/4012/11007)
**Владелец:** P3

## Симптом

DOM §4.4: `Node.nodeValue` для `Element`/`DocumentFragment` — всегда `null` (readonly-по-факту
геттер, сеттер — no-op). В Lumen у обоих типов свойство `nodeValue` попросту отсутствует в
дескрипторах — `element.nodeValue === undefined`, не `null`.

```js
var div = document.createElement('div');
div.nodeValue;                    // undefined, ожидается null
new DocumentFragment().nodeValue; // то же
```

`dom/nodes/Node-nodeValue.html`: `Element.nodeValue`/`DocumentFragment.nodeValue` подтесты FAIL
(`assert_equals: expected (object) null but got (undefined) undefined`) — независимо от
`CharacterData`/`ProcessingInstruction`, которые чинит BUG-1054.

## Что дальше

Не тронуто в BUG-1054 намеренно — другой класс дефекта (отсутствующий геттер, не пропущенная
`null`-коэрсия в сеттере) и другие два сайта в шиме. Нужен `nodeValue` геттер `{ return null; }`
(и no-op сеттер, спека явно требует no-op, не throw) на элементной обёртке и на
`DocumentFragment`, тем же паттерном, что уже есть у `DocumentType`/`Document`
(`web_api_shim_mid.js:3926`/`:4012`/`:11007`).

## Фикс

Аксессоры добавлены не на каждую обёртку, а в общий пакет `_LUMEN_WRAPPER_MEMBERS`
(`crates/js/src/shim/web_api_shim_mid.js`), имя `nodeValue` внесено в
`_LUMEN_NODE_MEMBER_NAMES`, поэтому геттер `{ return null; }` и no-op сеттер ставятся на
`Node.prototype` и достаются Element, DocumentFragment и ShadowRoot через цепочку
прототипов. У CharacterData/ProcessingInstruction/Attr/Document/DocumentType остаются свои
аксессоры с данными — они перекрывают биндинг `Node.prototype`.

**Тесты:** новый `crates/js/tests/cases/bug1074_node_value.rs`
(`cargo test -p lumen-js --features v8-backend --test all -- bug1074` — 3 passed: `null` у
`div`/`new DocumentFragment()`/`createDocumentFragment()`, no-op сеттер, `Text.nodeValue`
по-прежнему отдаёт и меняет данные). Обновлён существующий
`v8_core::wrapper_character_data_members_are_text_only` — он закреплял прежнее
`e.nodeValue === undefined`.
