# BUG-1032: HTML Tabular Data — element-specific IDL полностью отсутствует (`HTMLTableElement`/`HTMLTableSectionElement`/`HTMLTableRowElement`)

**Статус:** OPEN (ДОРАБОТКА → GAP-TABLEIDL)
**Тип:** нереализованная функциональность, не дефект реализованного кода — ведётся как задача
`GAP-TABLEIDL` в [ROADMAP.md](../ROADMAP.md), P3 как баг не берёт (`docs/probe-method.md` §8:
функциональности нет вовсе, объём — целая семья интерфейсов, не точечная правка).
**Найден:** P3 2026-09-07, побочно при локализации [BUG-1022](BUG-1022-OPEN.md)
(`html/semantics` — три `--check` подряд дают три разных набора регрессий)
**Компонент:** js (`crates/js/src/shim/*.js` — `HTMLTableElement`/`HTMLTableRowElement`/
`HTMLTableSectionElement` заведены только как теговые алиасы generic `HTMLElement`,
`grep -rn "insertRow\|insertCell\|createTHead\|createCaption" crates/js/src/shim/*.js` — ноль
совпадений кроме одного постороннего `'rows'` у другого элемента)

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
упомянутого в [BUG-1022](BUG-1022-OPEN.md)) — 0/3 сабтестов, 10/10 раз:

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
