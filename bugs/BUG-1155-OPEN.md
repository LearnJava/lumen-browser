# BUG-1155: `innerHTML` на `<tbody>`/`<tr>`/`<table>` разбирает фрагмент в режиме `in body`

**Статус:** OPEN
**Дата:** 2026-09-25
**Компонент:** html-parser (`crates/engine/html-parser/src/tree_builder.rs::new_fragment`
и `reset_insertion_mode` — стартовый insertion mode фрагмента безусловно `InBody`)
**Найден:** P3 2026-09-25, при закрытии [BUG-1022](BUG-1022-FIXED.md)

## Симптом

```html
<table><tr><td id=c1>a<td></table>
<script>
var tb = document.querySelector("table").firstElementChild;   // <tbody>
tb.innerHTML += "<tr><td><tr><td><tr><td><tr><td id=c2>";
</script>
```

Ожидается (HTML LS §13.4, контекст `<tbody>` → режим `in table body`): четыре
соседних `<tr>` внутри `<tbody>`. Lumen (`--dump-layout`, 2026-09-25):

```
<tr><td id="c1">a</td><td></td></tr><tr><td><tr><td><tr><td><tr><td id="c2"></td></tr></td></tr></td></tr></td></tr>
```

Каждый следующий `<tr>` вложен в `<td>` предыдущего: в режиме `in body` теги
`tr`/`td` без открытой таблицы — parse error «ignore», а парсер Lumen их не
игнорирует, а создаёт как обычные элементы. Глубина DOM растёт линейно с
числом строк.

## Следствия

* Любой `tbody.innerHTML = "<tr>…"` (частый приём перерисовки таблиц) даёт
  неверное дерево — строки вложены, `rows`/`cells` считаются неправильно.
* `html/semantics/tabular-data/processing-model-1/span-limits.html` вставляет
  так 65 532 строки: DOM-цепочка глубиной ~131 000 → `Maximum call stack size
  exceeded` в JS и `thread 'lumen-pipeline' has overflowed its stack` — abort
  всего процесса. Воспроизводится в `--dump-layout` уже с 10 000 строками
  (`exit=127`, `thread 'main' has overflowed its stack`); 1 000 строк проходят.
  Это то падение, которое через раннер WPT размножалось на соседние тесты
  (BUG-1022).

## Что нужно

§13.4 шаги 4–6: при контекстном элементе вместо «in body» выполнить «reset the
insertion mode appropriately» с контекстом на месте последнего узла стека
(`tbody`/`thead`/`tfoot` → `in table body`, `tr` → `in row`, `td`/`th` → `in
cell` не применяется для `last`, `table` → `in table`, `select` → `in select`,
`template` → текущий template mode, и т.д.). Сейчас `reset_insertion_mode`
при `last && is_fragment` жёстко возвращает `InBody`, а `new_fragment` ставит
`InBody` сразу. `FragmentContext` уже передаёт `local` контекстного элемента
(`dom_helpers.rs::parse_html_fragment_with_context`) — для `innerHTML=` данных
хватает; `insertAdjacentHTML`/`outerHTML` контекст пока не передают вовсе.

Отдельно (не этот баг): переполнение стека на глубоком DOM само по себе —
рекурсивный обход где-то в конвейере `lumen-pipeline`; после исправления
разбора `span-limits.html` его уже не достанет, но цепочка `<div>` той же
глубины достанет.
