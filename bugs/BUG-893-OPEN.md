# BUG-893 — `input.valueAsNumber`/`valueAsDate` не существуют вовсе, и присваивание им на нечисловом поле не бросает `InvalidStateError`, а молча проходит

**Статус:** OPEN
**Заведён:** 2026-08-23 (WPT-RUN-6, срез 29 — живой замер, вариант `input-value`)
**Область:** js (`grep -rn "valueAsNumber\|valueAsDate" crates/` — ноль совпадений во всём воркспейсе)
**Владелец:** P1/P3. Заведён P2 в ходе WPT-задачи, здесь не чинится.

## Симптом

`<input type=number value=3>.valueAsNumber` — `undefined` (ожидается `3`);
`<input type=date>.valueAsNumber`/`.valueAsDate` — тоже. Хуже того, HTML LS
§4.10.5.4 требует `InvalidStateError` при присваивании обоих на типе, который
их не поддерживает (`text`, `checkbox`) — присваивание проходит молча и
создаёт обычное expando-свойство.

Соседние точки того же интерфейса исправны: `stepUp()` двигает значение
(`3` → `4`), `setRangeText` работает, `validity` — объект. То есть это не
«форм нет», а ровно два отсутствующих аксессора.

## Прямое измерение

`tests/wpt/verify_cssom_svg_interface_gaps.py --variant input-value`
(2026-08-23, dev-release, Linux):

```
num-valueAsNumber = undefined     date-valueAsNumber = undefined
date-valueAsDate = undefined
text-valueAsNumber-throws = no-throw    text-valueAsDate-throws = no-throw
stepUp = 4    setRangeText = Xello    validity = object
```

## Цена по WPT

`html/semantics/forms/the-input-element/input-valueasnumber-invalidstateerr.html`
(остаток снимка WPT-RUN-5) плюс всё семейство `input-valueas*` той же папки.
Тест молчит целиком, без единой строки в логе: он ждёт исключения, которого
нет, а вердикт TIMEOUT берётся уже из хёрнесса.

## Что дальше

HTML LS §4.10.5.4 задаёт таблицу «тип → алгоритм преобразования»
(`number`/`range` — число, `date`/`month`/`week`/`time`/`datetime-local` — дата,
остальные — бросать). Аксессоры кладутся туда же, где уже живут
`stepUp`/`stepDown`, и переиспользуют их разбор значения.
