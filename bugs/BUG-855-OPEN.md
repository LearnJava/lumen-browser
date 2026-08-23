# BUG-855 — MutationObserver не видит половину скриптовых мутаций: `removeAttribute`, вставку через `insertBefore`/`replaceChild`, а `previousSibling`/`nextSibling` в записи всегда `null`

**Статус:** OPEN
**Заведён:** 2026-08-23 (WPT-RUN-6, срез 25 — живой замер, маркеры `mo-attributes`, `mo-childlist`, `mo-validation`)
**Область:** `crates/js/src/dom.rs` — обвязка `_mo_notify` (`dom.rs:9965`): перехвачены только `_lumen_append_child` (`:10058`), `_lumen_remove_child` (`:10067`), `_lumen_set_inner_html` (`:10046`), `_lumen_set_text_content` (`:10082`/`:10088`) и путь *установки* атрибута (`:10037`). Не перехвачены `_lumen_insert_before` (обёртка на `dom.rs:6638` есть, но только для сабресурсов), `replaceChild` и `removeAttribute` (`dom.rs:4250`, зовёт `_lumen_remove_attr` напрямую). Поля `nextSibling`/`previousSibling` записи захардкожены в `null` (`dom.rs:10001`–`10002`)
**Владелец:** P1/P3 (`lumen-js`). Заведён P2 в ходе WPT-задачи, здесь не чинится.
**Родственные:** [BUG-827](BUG-827-OPEN.md) — та же подсистема со стороны **парсера** (узлы, вставленные парсером, не порождают записей вовсе). Этот баг про **скриптовую** половину, которую BUG-827 явно записал как исправную.

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
