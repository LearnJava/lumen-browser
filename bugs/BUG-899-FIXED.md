# BUG-899 — User Timing L3: словарная форма игнорируется целиком — `mark`/`measure` теряют `detail`, `measure(name, {start, end})` меряет от начала страницы, у записей нет `toJSON`

**Статус:** FIXED 2026-09-23 (P3)
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

## Исправлено (2026-09-23, P3)

`crates/js/src/shim/performance_shim.js`, `Performance.prototype.mark`/
`measure`:

* `mark(name, opts)` читает `opts.detail` (по умолчанию `null`, WebIDL default
  для отсутствующего члена словаря), кладёт его в запись как есть — без
  структурного клонирования: страница получает тот же объект по ссылке,
  чего спецификация не запрещает для собственной реализации (клонирование
  важно только на границе `postMessage`, которой здесь нет);
* `measure(name, startOrMeasureOptions, endMark)` теперь различает две
  взаимоисключающие формы аргумента. Именованная (`start`/`end` — имя марки
  или число) не изменилась. Словарная (`{start, end, duration, detail}`)
  читается через общий `_perf_mark_to_timestamp` (та же конвертация «имя
  марки → её `startTime`» или «число → как есть», раньше жившая только в
  именованной ветке) и считает все четыре допустимые комбинации по §4.3:
  `start`+`end` напрямую, `start`+`duration` и `duration`+`end` — вычислением
  недостающей границы, только имя — `start=0`/`end=now()`;
* обе записи получили общий `toJSON()` (`_perf_user_timing_to_json`),
  возвращающий `{name, entryType, startTime, duration, detail}` — раньше
  `PerformanceEntry.prototype.toJSON` не существовал вовсе ни у одной записи
  этого шима.

Прямое измерение тем же пробником, что и в заведении
(`tests/wpt/verify_cssom_svg_interface_gaps.py --variant user-timing`,
dev-release):

```
mark-detail = 12/{"a":1}            (было 12/undefined)
measure-options = 10/{"x":1}        (было 122.868.../undefined)
entry-toJSON = function             (было undefined)
```

Именованная форма (`measure-names`), `measure-navtiming`,
`getEntriesByType`, `clearMarks`, `po-buffered` не изменили поведение —
подтверждено тем же прогоном.

`cargo clippy -p lumen-js --all-targets --features v8-backend -- -D
warnings` чист. `scripts/scoped-test.sh` зелёный (весь затронутый граф
пакетов, включая `lumen-driver`). Правка не трогает layout/paint — только
данные, текущие в JS-объекте, — поэтому вместо полного `graphic_tests/run.py`
показан пустой `dump_golden.py` (12/12 дампов совпадают с эталоном).

Не в скоупе: `SyntaxError`/`TypeError` на невалидных комбинациях аргументов
(например, все три из `start`/`duration`/`end` разом) — то же различение
«молча даёт неверный результат» vs «не бросает», что и у
[BUG-696](BUG-696-OPEN.md), здесь не тронуто.
