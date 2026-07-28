# BUG-325: `CharacterData.appendChild()` не бросает `HierarchyRequestError`; `ProcessingInstruction` вообще не имеет `appendChild`

**Статус:** FIXED 2026-07-20
**Дата:** 2026-07-20
**Компонент:** js (WEB_API_SHIM, `crates/js/src/dom.rs`)
**Найден:** `docs/wpt-status.md` / `.tmp/wpt-report-all.html`, WPT `dom/nodes/CharacterData-appendChild.html` (0/9 сабтестов).

## Симптом

По DOM Standard §4.2.3 (pre-insert validity), `Node.appendChild()`/`insertBefore()` должны
бросать `HierarchyRequestError`, если `this` (родитель) не `Document`/`DocumentFragment`/`Element` —
`Text`/`Comment`/`ProcessingInstruction` (все — `CharacterData`) детей иметь не могут в принципе.

Два независимых дефекта дают одну и ту же красную таблицу:

1. **`Text`/`Comment`** (обёрнуты через `_lumen_make_element`, `crates/js/src/dom.rs:5360`,
   общий литерал `appendChild`) — метод `appendChild` ничего не проверяет про тип получателя,
   просто вызывает `_lumen_append_child(nid, c.__nid__)` для любого `nid`, включая узлы
   Text/Comment. Отсюда `assert_throws_dom: ... did not throw` — 6 сабтестов
   (`Text.appendChild(Text/Comment/ProcessingInstruction)`,
   `Comment.appendChild(Text/Comment/ProcessingInstruction)`).

2. **`ProcessingInstruction`** (`_lumen_make_processing_instruction`, `crates/js/src/dom.rs:4350`) —
   объектный литерал `pi` вообще не определяет `appendChild` (только геттеры
   `nodeType`/`nodeName`/`target`/`data`/`nodeValue`/`textContent`/`length`/`ownerDocument`/
   `parentNode`/`childNodes`), и на цепочке прототипов (`ProcessingInstruction.prototype` →
   `CharacterData.prototype` → `Node.prototype`, все — пустые abstract-base объекты,
   `crates/js/src/dom.rs:4392`–4449) метода тоже нет. Вызов падает как
   `TypeError: node1.appendChild is not a function` вместо ожидаемого
   `DOMException HierarchyRequestError` — 3 сабтеста
   (`ProcessingInstruction.appendChild(Text/Comment/ProcessingInstruction)`).

Итог: `dom/nodes/CharacterData-appendChild.html` — гарнес `OK` (значит, только что все 9
сабтестов честно выполнились до конца), но **0/9 сабтестов PASS**.

## Ожидание

- Общий `appendChild` в `_lumen_make_element` (и, вероятно, `insertBefore`/`replaceChild`/
  `insertAdjacentElement` — не проверено, вне скоупа этого репро) должен бросать
  `new DOMException('...', 'HierarchyRequestError')`, когда `this` — Text/Comment/CDATASection
  (любой CharacterData-подтип, не только не-нативные PI).
- `ProcessingInstruction`-объекту нужен собственный (или на прототипе) `appendChild`/`insertBefore`/
  `replaceChild`/`removeChild`, бросающий тот же `HierarchyRequestError` — сейчас там в принципе
  нет ни одного из insertion-методов Node, так что любой вызов даёт неверный тип ошибки
  (`TypeError`, а не `DOMException`).

## Замечание

Оба дефекта воспроизводятся на общей причине: в проекте нет общей точки валидации
"parent must be Document/DocumentFragment/Element" для семейства `Node`-insertion методов —
`appendChild` у Element-обёрток и у detached-PI реализованы независимо друг от друга и оба её не
делают. Чинить, вероятно, стоит вместе (общий helper), а не по одному сабтесту.

## Фикс

Общий хелпер `_lumen_character_data_insertion_error()` (`crates/js/src/dom.rs`, рядом с
`_lumen_make_processing_instruction`) строит `DOMException(..., 'HierarchyRequestError')`.
Используется в двух точках:

- Общий `appendChild` (`_lumen_build_element`, `crates/js/src/dom.rs:5360`) — в начале метода
  добавлена проверка `if (_lumen_is_text_node(nid)) throw ...`, бросающая до попытки что-либо
  вставить. Покрывает `Text`/`Comment` (оба сейчас реализованы через один и тот же native-текстовый
  узел под капотом — `document.createComment` строит текстовый узел, отдельного представления
  `NodeData::Comment` в JS-обёртке пока нет; это отдельный, более широкий гэп, не в скоупе этого бага).
- `ProcessingInstruction`-объект (`_lumen_make_processing_instruction`) получил свои
  `appendChild`/`insertBefore`/`replaceChild`/`removeChild` — все четыре бросают ту же ошибку вместо
  прежнего `TypeError: ... is not a function` (у PI не было вообще ни одного insertion-метода).

Регрессионный юнит-тест `character_data_append_child_throws_hierarchy_request_error`
(`crates/js/src/dom.rs`, рядом с `character_data_prototype_chain`) зеркалит все 9 комбинаций
Text/Comment/ProcessingInstruction из самого WPT-теста, проверяя `e.name === 'HierarchyRequestError'
&& e.code === 3`. Прогнан против V8 (`--features v8-backend`, дефолтный движок по ADR-018) — зелёный.

`insertBefore`/`replaceChild`/`insertAdjacentElement` для Element-обёрток (не CharacterData) —
осознанно не тронуты, как и было указано в разделе «Ожидание»: WPT-тест их не проверяет, и это
отдельный необследованный периметр.
