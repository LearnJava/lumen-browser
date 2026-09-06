# BUG-1023 — `:heading()`/`:heading(N)` селектор и `headingOffset`/`headingReset` не реализованы

**Статус:** OPEN
**Заведён:** 2026-09-06 (P2, WPT-RUN-7 срез 19 — `html/semantics/sections`)
**Область:** css-parser (селекторы — нет `:heading`/`:heading(N)`) + dom (нет IDL-отражения
`headingOffset`/`headingReset` контент-атрибутов на `HTMLElement`)

## Симптом

`headingoffset-and-headingreset.html` и `headingoffset-mutations.html` — оба файла
целиком FAIL (0/100 сабтестов пройдено на категорию из двух файлов), второй ещё и
`expected: ERROR` на уровне харнесса целиком:

```
FAIL headingoffset (set via attribute) should change the level a heading matches against
FAIL case 1: heading level for <h1 data-expected-offset="2">...</h1> should match based on
  expected document structure
```

## Причина

Оба теста реализуют новый черновик WHATWG (`whatwg/html#11086`, ссылка в тесте) —
вычисляемый «уровень заголовка» (`heading level`) с учётом вложенных контейнеров
`headingoffset="N"`/`headingreset`, доступный через:
- CSS-псевдокласс `:heading` / `:heading(N)` (matches любой `<h1>`–`<h6>` на своём
  эффективном уровне после применения offset/reset цепочки предков);
- IDL-атрибуты `HTMLElement.headingOffset` (число) / `HTMLElement.headingReset`
  (boolean), отражающие одноимённые контент-атрибуты.

Ни то, ни другое не реализовано: `:heading()` не существует как псевдокласс в
css-parser (значит `el.matches(":heading(1)")` кидает `SyntaxError` вместо булева
результата — источник `ERROR` на харнесс), а `headingOffset`/`headingReset` на
`HTMLElement` не определены вовсе (`modal.headingReset` читает `undefined`, не
булево — источник провала остальных ассертов).

## Почему это важно

Фича экспериментальная и очень новая (черновик 2024/2025, ссылка на PR, не RFC) —
не блокер реального контента, но полностью нулевое покрытие (0/100) означает, что
`--check` на этой категории не отловит вообще никакую регрессию до тех пор, пока
фича не появится хотя бы частично.

## Возможный путь фикса

Два независимых куска: (1) псевдокласс `:heading`/`:heading(N)` в
`css-parser`/селекторный матчер (эффективный уровень = базовый уровень тега h1-h6
минус 1 плюс сумма `headingoffset` всех предков-контейнеров до ближайшего
`headingreset`/модального `<dialog>`, клампится в [0, 9]); (2) IDL-отражение
`headingOffset`/`headingReset` в JS-шиме аналогично другим числовым/булевым
content-attribute reflection-полям. Оценка объёма не делалась — не смотрелось,
есть ли уже общий helper для «эффективного уровня заголовка» где-то в a11y-дереве
(`crates/engine/a11y`), которым можно было бы переиспользовать вычисление.

## Воспроизведение

```
tests/wpt/.venv/bin/python3 tests/wpt/run_report.py \
  --binary target/dev-release/lumen --all --root html/semantics/sections \
  --recursive --update-expected
```

## Не проверялось

Остальные `html/semantics/*` под-пути на предмет того же класса (другие новые
селекторы/IDL-отражения) — не искалось системно, найдено только на этих двух
файлах.
