# BUG-899 — User Timing L3: словарная форма игнорируется целиком — `mark`/`measure` теряют `detail`, `measure(name, {start, end})` меряет от начала страницы, у записей нет `toJSON`

**Статус:** OPEN
**Заведён:** 2026-08-23 (WPT-RUN-6, срез 29 — живой замер, вариант `user-timing`)
**Область:** js (`crates/js/src/dom.rs:8366-8392` — `performance.mark`/`performance.measure` в `WEB_API_SHIM`)
**Владелец:** P1/P3. Заведён P2 в ходе WPT-задачи, здесь не чинится.

## Симптом

Три отдельные потери в одной паре методов:

* `performance.mark(name, {detail})` — `startTime` из словаря учитывается
  (в замере `12`), а `detail` теряется: у записи он `undefined`;
* `performance.measure(name, {start: 0, end: 10, detail})` — словарь не
  читается вовсе: `duration` вышел `122.87` (то есть от начала документа до
  «сейчас»), а не `10`;
* ни у одной записи нет `toJSON()`, хотя `PerformanceEntry` его требует.

Именованная форма (`measure(name, startMark, endMark)`), `getEntriesByType`,
`clearMarks` и `PerformanceObserver` с `buffered: true` при этом работают —
дефект ровно в разборе аргумента-словаря.

Отличается от соседей: [BUG-687](BUG-687-OPEN.md) — про то, что записи не
являются `PerformanceMark`/`PerformanceMeasure` (идентичность прототипа),
[BUG-696](BUG-696-OPEN.md) — про то, что не бросаются `SyntaxError`/`TypeError`
на неверных аргументах. Здесь корректный по спецификации вызов молча даёт
неверный результат.

## Прямое измерение

`tests/wpt/verify_cssom_svg_interface_gaps.py --variant user-timing`
(2026-08-23, dev-release, Linux):

```
mark = object            mark-detail = 12/undefined
measure-names = object   measure-options = 122.868408203125/undefined
measure-navtiming = object
getEntriesByType = 3/2   entry-toJSON = undefined   clearMarks = 0
po-fired 1               po-buffered = observing
```

## Цена по WPT

**Не измерена, и это проверено, а не пропущено.** Соседство напрашивалось:
в остатке снимка WPT-RUN-5 три id `user-timing`/`performance-timeline`
(`measure.html`, `measure_navigation_timing.html`,
`performance-timeline/po-mark-measure.any.html`) — но `grep` по их исходникам
показывает только ИМЕНОВАННУЮ форму (`performance.measure(name)`,
`(name, startMark)`, `(name, startMark, endMark)`), которая здесь как раз
работает. Значит эти три висят по другой причине и остаются в остатке;
приписать их сюда было бы тем самым «неверным `ref`», который срез 26 учил не
делать. Дефект найден живым замером, механизма в `timeout_audit.py` не
получает — маркера, который был бы про него, а не про соседа, у него нет.

## Что дальше

User Timing L3 §3.1/§3.3: `markOptions.detail` кладётся в запись как есть
(структурно клонированное значение), `measureOptions` даёт четыре формы
(`start`+`end`, `start`+`duration`, `duration`+`end`, только имя), и
`toJSON()` возвращает собственные перечислимые поля записи. Правка целиком в
шиме, на готовых данных.
