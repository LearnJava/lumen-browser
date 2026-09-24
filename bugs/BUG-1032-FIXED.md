# BUG-1032: HTML Tabular Data — element-specific IDL полностью отсутствует (`HTMLTableElement`/`HTMLTableSectionElement`/`HTMLTableRowElement`)

**Статус:** FIXED (обнаружено уже закрытым) 2026-09-25 (P1, `GAP-TABLEIDL`)
**Тип:** заявка описывала нереализованную функциональность и была верно переклассифицирована в
задачу `GAP-TABLEIDL` в [ROADMAP.md](../ROADMAP.md), но сама реализация внесена коммитом
`d5fbda1b4d` (2026-09-15, «BUG-581: полный HTML LS §4.9.11 API для
`<table>`/`<tr>`/`<thead>`/`<tbody>`/`<tfoot>`») — независимо и до того, как заявка BUG-1032
(найдена 2026-09-07) была превращена в этот `GAP`. Сам `GAP-TABLEIDL` никогда не сверялся с
кодом после слияния BUG-581 и оставался помечен `planned` три с половиной недели.
**Найден:** P3 2026-09-07, побочно при локализации [BUG-1022](BUG-1022-FIXED.md)
(`html/semantics` — три `--check` подряд дают три разных набора регрессий)
**Компонент:** js (`crates/js/src/shim/web_api_shim_mid.js`/`web_api_shim_tail_b.js` —
`HTMLTableElement`/`HTMLTableRowElement`/`HTMLTableSectionElement` полный HTML §4.9.11 API;
`crates/js/tests/cases/bug581_table_api.rs` — 17 тестов, все зелёные)

## Механизм

`web_api_shim_mid.js` заводит `HTMLTableElement`/`HTMLTableRowElement`/
`HTMLTableCellElement`/`HTMLTableSectionElement` как теговые конструкторы
(`'TABLE': HTMLTableElement`, `'TR': HTMLTableRowElement`, …), но ни один из
них не получает НИ ОДНОГО специфичного для интерфейса IDL-члена по HTML §4.9
Tabular data. Элемент `<table>`/`<tr>`/`<thead>`/… ведёт себя как обычный
`HTMLElement` — только теговое имя отличается.

Отсутствуют целиком:

- **`HTMLTableElement`** (§4.9.1): `caption`/`createCaption()`/`deleteCaption()`,
  `tHead`/`createTHead()`/`deleteTHead()`, `tFoot`/`createTFoot()`/`deleteTFoot()`,
  `tBodies`, `rows`, `insertRow(index)`, `deleteRow(index)`, `createTBody()`.
- **`HTMLTableSectionElement`** (§4.9.5, общий для `<thead>`/`<tbody>`/`<tfoot>`):
  `rows`, `insertRow(index)`, `deleteRow(index)`.
- **`HTMLTableRowElement`** (§4.9.8): `rowIndex`, `sectionRowIndex`, `cells`,
  `insertCell(index)`, `deleteCell(index)`.

Все три — специфичные live-`HTMLCollection`/`insert-at-index` алгоритмы
(`rows`/`cells` — не просто `querySelectorAll`, а фильтр по прямому/непрямому
потомку с конкретным тегом и конкретной вложенностью; `insertRow` без
`tHead`/`tBodies` обязан САМ создать `<tbody>`), а не единственная точечная
правка — тот же класс, что уже реклассифицированные BUG-505/507/511/521/536
(`docs/probe-method.md` §8).

## Симптом

Живой прогон (`run_smoke.py`, 10 повторов подряд — детерминировано,
**не** флак): `insertRow-method-02.html` (часть общего знаменателя,
упомянутого в [BUG-1022](BUG-1022-FIXED.md)) — 0/3 сабтестов, 10/10 раз:

```
FAIL table should start out empty - Cannot read properties of undefined (reading 'length')
FAIL insertRow should insert a tr element - table.insertRow is not a function
FAIL insertRow(): Empty table - Cannot read properties of undefined (reading 'parentNode')
```

Корень — `table.insertRow is not a function` (метод не существует вовсе);
первый и третий FAIL — каскад от него же (`tr` остаётся `undefined`, второй
independent FAIL — `table.rows`, судя по всему тоже `undefined`, читается до
`insertRow`).

## Масштаб

18 файлов только в `html/semantics/tabular-data/` явно зовут один из
перечисленных членов (`grep -rlE "insertRow\(|deleteRow\(|\.rows\b|tHead\b|
tFoot\b|tBodies\b|insertCell\(|deleteCell\(|\.cells\b|createTHead|createTFoot|
createCaption|createTBody"`), не считая `tables.html`/DOM-обходов в других
категориях, которые могут читать `rows`/`cells` попутно.

## Связь с BUG-1022

Этот файл — один из трёх общих знаменателей исходной находки BUG-1022
(«три `--check` подряд на `html/semantics` дают три разных набора
регрессий»). Локализация показала: **этот конкретный файл не флаки** — 10/10
повторов дают идентичный, детерминированный результат с ясным корнем
(отсутствующий метод). Не объясняет:
- почему СОСЕДНИЕ файлы той же папки (`table-rows.html`, `table-insertRow.html`,
  `tHead.html`) переключались OK→ERROR по-разному между тремя прогонами
  BUG-1022 (гипотеза: TypeError от отсутствующего метода в одном файле мог
  где-то давить на состояние процесса/воркера, влияя на СЛЕДУЮЩИЙ тест в том
  же прогоне — не проверено, `--processes 6` параллелит по процессам, так что
  внутрипроцессное состояние не должно течь между разными файлами; более
  вероятная гипотеза — независимая причина, не связанная с этим багом);
- второй общий знаменатель, `autoplay.html` (OK→ERROR), и растущий
  TIMEOUT-кластер `forms/form-submission-0/*` — оба вне `tabular-data/`,
  этим багом не покрываются.

BUG-1022 остаётся `OPEN` — эта находка закрывает один из двух общих
знаменателей, не саму загадку роста счётчика регрессий.

## Закрытие 2026-09-25 (P1)

Живой прогон юнит-тестов на `main` (`cargo test -p lumen-js --features v8-backend
bug581_table_api`) дал 17/17 зелёных — реализация присутствует полностью:
`rows`/`tBodies`/`cells` (live-`HTMLCollection` на `_lumen_make_nid_collection`),
`caption`/`tHead`/`tFoot` (геттеры/сеттеры с `TypeError`/`HierarchyRequestError`),
`createCaption`/`createTHead`/`createTFoot`/`createTBody`/`delete*`,
`insertRow`/`deleteRow` (на `HTMLTableElement` и `HTMLTableSectionElement`),
`insertCell`/`deleteCell`/`cellIndex`, `rowIndex`/`sectionRowIndex`. Внесено
коммитом `d5fbda1b4d` («BUG-581: полный HTML LS §4.9.11 API…», 2026-09-15) —
раньше, чем эта заявка была переклассифицирована в `GAP-TABLEIDL`. `git log`
подтверждает: `GAP-TABLEIDL` создан по замеру, снятому ДО коммита `d5fbda1b4d`,
и статус документации никогда не сверялся с кодом заново. Изменений кода в
этой сессии нет — только исправление статуса `BUGS.md`/`ROADMAP.md`/этого файла.
Остаток вне скоупа (не проверен заново, унаследован из BUG-581): namespace-
схлопывание [BUG-830](BUG-830-OPEN.md), prefix-упрощение [BUG-367](BUG-367-FIXED.md),
кросс-realm `instanceof` через `<iframe>`, `DOMParser`+`importNode`,
`colSpan`/`rowSpan` reflection — ни одна из этих причин не относится к самому
API-поверхности `GAP-TABLEIDL`.
