# BUG-855 — MutationObserver не видит половину скриптовых мутаций: `removeAttribute`, вставку через `insertBefore`/`replaceChild`, а `previousSibling`/`nextSibling` в записи всегда `null`

**Статус:** FIXED 2026-09-23 (P3)
**Заведён:** 2026-08-23 (WPT-RUN-6, срез 25 — живой замер, маркеры `mo-attributes`, `mo-childlist`, `mo-validation`)
**Область:** `crates/js/src/shim/web_api_shim_mid_b2.js` — обвязка `_mo_notify`. Заявка указывала на `crates/js/src/dom.rs` (`:9965`, `:10001`–`10002`) — эти строки устарели: подсистема давно переехала в шим-файл (SPLIT-JS3, 2026-08-28), сама диагностика осталась верной.
**Владелец:** P1/P3 (`lumen-js`). Заведён P2 в ходе WPT-задачи.
**Родственные:** [BUG-827](BUG-827-FIXED.md) — та же подсистема со стороны **парсера** (узлы, вставленные парсером, не порождают записей вовсе). Этот баг про **скриптовую** половину, которую BUG-827 явно записал как исправную.

## Симптом

Наблюдатель, поставленный на элемент, молчит ровно про те мутации, которые
WPT-файлы `dom/nodes/MutationObserver-*.html` проверяют первыми, — и тест
ждёт колбэка до таймаута раннера:

```js
mo.observe(n, { attributes: true, attributeOldValue: true });
n.setAttribute('data-x', '1');   // запись есть
n.removeAttribute('data-x');     // записи НЕТ
```

```js
mo.observe(p, { childList: true });
p.insertBefore(c, b);            // записи НЕТ
p.removeChild(a);                // запись есть, но addedNodes/removedNodes без соседей
p.replaceChild(i, c);            // только удаление, добавление потеряно
```

## Прямое измерение

`tests/wpt/verify_focus_mutation_animation_gaps.py --variant mo-attributes
--variant mo-childlist --variant mo-validation` (2026-08-23, dev-release,
Linux, `main` = `530d0a444`, `--seconds 5`, страница жива — 9 тиков):

| мутация | ожидалось | получено |
|---|---|---|
| `setAttribute('data-x','1')` | запись `attributes` | ✔ `attributes:data-x:null` |
| `setAttribute('class','c2')` | запись + `oldValue="c1"` | ✔ |
| `removeAttribute('data-x')` | запись `attributes` | **ничего** |
| `id = 'n2'` | запись + `oldValue="n"` | ✔ |
| `className = 'c3'` | запись + `oldValue="c2"` | ✔ |
| `insertBefore(c, b)` | `addedNodes=[c]`, `previousSibling=a` | **ничего** |
| `removeChild(a)` | `removedNodes=[a]`, соседи | запись есть, `prev=null next=null` |
| `replaceChild(i, c)` | `+[i] -[c]` | только `-1`, `addedNodes` пуст |
| `appendChild` (subtree-наблюдатель) | `+1 -0` | ✔ `+1 -0 target=b` |
| `takeRecords()` | 2 | ✔ 2 |
| `observe(document, {})` | `TypeError` | **no-throw** |
| `new MutationObserver()` без колбэка | `TypeError` | **no-throw** |

Итог: из 5 атрибутных мутаций записаны 4, из 3 childList-мутаций записана
одна и та не полностью. `characterData` (отдельный вариант `mo-characterdata`)
исправен целиком, включая `characterDataOldValue`.

## Причина (локализована чтением кода)

`_mo_notify` вызывается из обёрток вокруг четырёх примитивов. `insertBefore`
в этот список не входит: его обёртка (`dom.rs:6638`) навешена ради
`_lumen_resource_after_insert`, а уведомления наблюдателей не делает.
`removeAttribute`/`removeAttributeNS` зовут `_lumen_remove_attr` мимо
перехваченного пути установки. `replaceChild` реализован поверх
remove+insert, поэтому теряет ровно свою вставку. `nextSibling`/
`previousSibling` записи не вычисляются вовсе — литерал записи содержит
`null` (`dom.rs:10001`–`10002`).

## Масштаб

Механизм `mutation-record-missing` в `tests/wpt/timeout_audit.py` — **9 id**
остатка снимка WPT-RUN-5 (`dom/nodes/MutationObserver-attributes.html`,
`-characterData.html`, `-childList.html`, `-sanity.html`, `-textContent.html`,
`Node-insertBefore.html`, `ParentNode-append.html`, `-prepend.html`,
`-replaceChildren.html`). Имена зависших подтестов называют дефект прямо:
`attributes Element.removeAttribute: removal mutation`,
`childList Node.insertBefore: addition mutation`,
`childList Node.insertBefore: removal and addition mutations`.

За пределами WPT это тихая потеря событий для любого кода, который следит за
DOM через `MutationObserver` (фреймворки, аналитика, наш собственный
knowledge-слой).

## Направление починки (не предписание)

Перенести уведомление на общий уровень примитивов вставки/удаления (одна
точка на `insert`/`remove`, как это уже сделано для сабресурсов в
`_lumen_resource_after_insert`), добавить обёртку на `_lumen_remove_attr` и
вычислять соседей на момент мутации. Проверки аргументов `observe()`
(`DOM §4.3.1`, шаг 3: ни один из `childList`/`attributes`/`characterData` не
`true` → `TypeError`) — отдельная строка там же.

## Как проверить фикс

1. `tests/wpt/.venv/bin/python tests/wpt/verify_focus_mutation_animation_gaps.py
   --variant mo-attributes --variant mo-childlist --variant mo-validation` —
   ожидается `mo-attr-callback n=5`, `mo-cl-callback n=3` с непустыми
   `addedNodes` и соседями, `mv-empty-init TypeError`.
2. WPT: `run_report.py --all --root dom/nodes --recursive` (файлы
   `MutationObserver-*`).

## Исправление

`_lumen_remove_attr` обёрнута тем же паттерном, что и `_lumen_set_attr`
(читает старое значение до вызова натива, зовёт `_mo_notify` после). Новая
обёртка над `_lumen_insert_before` добавлена рядом с уже существовавшими
обёртками `_lumen_append_child`/`_lumen_remove_child` — все три теперь читают
соседей через общий `_lumen_mo_siblings(parentNid, nid)` (после вставки — для
`insertBefore`/`appendChild`, до удаления — для `removeChild`, иначе список
уже не содержит узел).

`Node.replaceChild` — отдельный случай: он реализован поверх insert+remove,
и после того как оба этих натива стали слать собственную запись, наивный
повторный вызов той же пары дал бы ДВЕ записи вместо одной комбинированной
`+added -removed`, которую требует DOM §4.2.4 "replace" (WPT
`MutationObserver-childList.html`, `n50`: одна запись `removedNodes:[old],
addedNodes:[new]`). Функция переопределена так, чтобы звать «сырые»
(домоционные, ещё не обёрнутые в MO) версии натива и слать ровно один
`_mo_notify`. Ловушка при переопределении: `_LUMEN_WRAPPER_MEMBERS` — только
исходный словарь методов; `web_api_shim_mid.js` уже успевает снять с него
`Object.getOwnPropertyDescriptors` в `_LUMEN_WRAPPER_DESCRIPTORS` ДО того, как
этот файл выполняется, и именно этот снимок (не словарь) расходится по всем
интерфейс-прототипам через `_lumen_wrapper_proto_for`. Правка одного словаря
без правки снимка компилируется и не бросает ошибку, но не меняет ни одного
реального `replaceChild` — обе ссылки обновлены.

`MutationObserver` конструктор и `observe()` получили недостающую валидацию
(DOM §4.3.1): отсутствующий колбэк и `observe(target, {})` без единого
включённого вида мутации теперь бросают `TypeError`; `characterDataOldValue`
без явного `characterData` по-прежнему подразумевает его (как уже было для
`attributeOldValue`/`attributes`).

Не в скоупе (осталось как было): `replaceChild` на самозамене/внутренней
перестановке одного и того же узла (WPT `n52`/`n53` того же файла — там
ожидаются иные, более тонкие комбинации записей) и `previousSibling`/
`nextSibling` при wholesale-замене через `innerHTML =`/`textContent =` (всё
ещё `null` — WPT для этих кейсов их и не проверяет).

Проверено: `verify_focus_mutation_animation_gaps.py --variant mo-attributes
--variant mo-childlist --variant mo-validation` — все три совпали с
ожиданием построчно. `run_report.py` не удалось прогнать в этой среде
(`wss`-сервер падает на Python 3.14 — известная непричастная проблема, не
эта правка). 6 новых юнит-тестов в `crates/js/src/dom/tests/v8_perf_observers.rs`.
