# BUG-1064 — `getHTML()` не сериализует теневые корни, а `ShadowRoot.serializable`/`clonable`/`delegatesFocus`/`slotAssignment` не существуют: 6528 FAIL-подтестов `shadow-dom/declarative/gethtml.html`

**Статус:** OPEN
**Тип:** недоделанная реализация — `getHTML` в шиме прямо помечен заглушкой («Phase 0: serializableShadowRoots option deferred»); полей `serializable`/`clonable`/`delegatesFocus`/`slotAssignment` у `ShadowRoot` нет. Возможно, по классификации P3 это ДОРАБОТКА, а не дефект — решение за ним.
**Заведён:** 2026-09-19 (WPT-RUN-7 срез 36, категория `shadow-dom`)
**Область:** js-шим — `crates/js/src/shim/web_api_shim_mid.js:3857` (`ShadowRoot.prototype.getHTML`, игнорирует `opts`) и `:7848` (`getHTML` в литерале `Element`, тоже игнорирует `opts`); `_lumen_make_shadow_root` (`crates/js/src/dom.rs:1315`) — сюда `attachShadow`-словарь не доносится.
**Владелец:** P3 (P2 багов не чинит).

## Симптом

Проба вне WPT (`--dump-layout`; страница вызывает API и пишет результат в DOM):

```js
var s = h.attachShadow({mode:'closed', delegatesFocus:true, clonable:true, serializable:true});
s.mode            // 'closed'          — верно
s.host            // [object HTMLDivElement] — верно
s.delegatesFocus  // undefined         — ожидание true
s.clonable        // undefined         — ожидание true
s.serializable    // undefined         — ожидание true
s.slotAssignment  // undefined         — ожидание 'named'
```

```js
var s = h.attachShadow({mode:'open', serializable:true}); s.innerHTML = '<b>x</b>';
h.getHTML({serializableShadowRoots:true})  // ''  — ожидание '<template shadowrootmode="open" shadowrootserializable=""><b>x</b></template>'
h.getHTML({shadowRoots:[s]})               // ''  — то же ожидание
s.getHTML()                                // '<b>x</b>' — верно
```

`getHTML` у обычного элемента при этом есть (`typeof h.getHTML === 'function'`), а
`Object.hasOwn(Element.prototype, 'getHTML')` — `false` (метод не лежит на `Element.prototype`,
где его помещает WebIDL; проба не выяснила, на каком объекте цепочки он живёт).

## Масштаб

`shadow-dom/declarative/gethtml.html` — **6528 FAIL-подтестов** (3264 `Element.getHTML() on …`
+ 3264 `ShadowRoot.getHTML() on …`, параметризация по `mode`/`delegatesFocus`/`serializable`/
`clonable`/содержимому дерева), то есть ~78 % всех непройденных подтестов категории `shadow-dom`
(6528 из 8380 = 9689 − 1309 PASS в срезе 36). Файл — самодостаточный, без `test_driver`, упирается именно в
эти два пробела; сколько из 6528 упадёт после починки одного из них, а сколько потребует
обоих, не измерено.

## Ожидание

`attachShadow(init)` сохраняет `delegatesFocus`/`clonable`/`serializable`/`slotAssignment` и
отдаёт их геттерами `ShadowRoot`; `getHTML({serializableShadowRoots, shadowRoots})` на элементе
и на `ShadowRoot` сериализует теневые корни хоста как `<template shadowrootmode=…
[shadowrootdelegatesfocus] [shadowrootserializable] [shadowrootclonable]>` (HTML LS §13.3
«serializing HTML fragments»). Смежный `ShadowRoot.prototype.cloneNode` (`web_api_shim_mid.js:3863`)
безусловно бросает `NotSupportedError` — при `clonable:true` спека требует клонирования;
это отдельная проверка после появления поля `clonable`.

После починки baseline `tests/wpt/metadata/shadow-dom/declarative/gethtml.html.ini` (6528
записей `expected: FAIL`) станет массой unexpected-pass — регенерировать `--update-expected`
и подтвердить тремя `--check` подряд.
