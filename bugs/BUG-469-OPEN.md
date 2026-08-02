# BUG-469: zero-width new-formatting-context box not positioned into a zero-width gap between floats

**Статус:** OPEN
**Дата:** 2026-08-02
**Компонент:** layout (float positioning, CSS2.1 §9.5.1)
**Найден:** WPT-RUN-1 (`docs/wpt-status.md`, точечная проверка на 4 файлах,
2026-08-02) — формализовано в BUG-NNN во время WPT-RUN-3 срез 2

## Симптом

`css/CSS2/floats/zero-space-between-floats-00{1..4}.html` — все 4 файла,
1/1 сабтест FAIL каждый:

```
FAIL <case> - assert_equals:
undefined
offsetLeft expected 100 but got 0
```

Каждый тест зажимает нулевой ширины блок (`overflow:hidden; width:0`,
новый formatting context) между двумя float'ами (либо после float + clearance)
так, что для него остаётся ровно нулевой ширины "щель" в потоке. Спека
(9.5.1) требует разместить такой блок именно в эту щель — тестируемый
`offsetLeft`/`offsetTop` указывает на её позицию (100px после левого float,
или ниже float'ов при `clear`). Lumen вместо этого кладёт блок в (0, 0) —
как будто float'ы вообще не влияют на его позицию.

Обнаружено ещё во время точечной проверки WPT-RUN-1 (см. `docs/wpt-status.md`
→ строка `css`, "Точечная проверка... подтверждает DoD: хелпер теперь
выполняется и даёт содержательные фейлы с числами... похоже на реальный
дефект позиционирования float с нулевым зазором"), тогда не заводился
отдельным BUG-NNN — заведён сейчас как часть группировки провалов
(WPT-RUN-3 срез 2).

## .ini

`tests/wpt/metadata/css/CSS2/floats/zero-space-between-floats-{001,002,003,004}.html.ini`
— по одному сабтесту `expected: FAIL` в каждом.
