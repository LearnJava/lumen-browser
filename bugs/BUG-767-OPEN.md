# BUG-767 — `performance.timing` / `performance.navigation` (legacy Navigation Timing L1) отсутствуют

**Статус:** OPEN
**Компонент:** js (`crates/js/src/dom.rs` — секция `// ── performance (HR Timer …`, `Performance.prototype`)
**Найден:** P3 при закрытии [BUG-400](BUG-400-FIXED.md), 2026-08-11

## Симптом

`tests/wpt/hr-time/performance-tojson.html` — единственный тест,
названный заявкой BUG-400 под пункт `toJSON()`. После фикса BUG-400 он
проходит первое утверждение (`typeof performance.toJSON === 'function'`)
и падает на следующем:

```
FAIL Test performance.toJSON() - assert_equals: expected "function" but got "undefined"
  (typeof(timing.toJSON), где timing = performance.timing)
```

`performance.timing` и `performance.navigation` — `undefined`. Грепом по
`crates/` имена `PerformanceTiming`/`PerformanceNavigation` не
встречаются ни разу.

## Причина

Legacy-интерфейсы Navigation Timing L2 §5–6 (`PerformanceTiming`,
`PerformanceNavigation`, выставляемые partial-атрибутами `Performance`)
не реализованы вовсе. Формально это отдельная от
[BUG-640](BUG-640-OPEN.md) поверхность — там речь о современной
`PerformanceNavigationTiming`-записи (L2, `getEntriesByType('navigation')`),
здесь о двух легаси-атрибутах самого `performance`, — но **корень у них
один**: снимков вех навигации в движке нет ни одного.
`deliver_nav_timing` (`crates/shell/src/main.rs`) шлёт
`_lumen_deliver_perf_entry('navigation', url, 0.0, duration_ms, null)` —
URL и суммарную длительность, `detail_json` всегда `null`.

Поэтому `Performance.prototype.toJSON` (BUG-400) сериализует один
`timeOrigin`: подставить 21 нулевую веху (`navigationStart`…
`loadEventEnd`) означало бы позеленить тест, оставив фичу сломанной.

## Что нужно сделать

**Порядок обязателен: сначала [BUG-640](BUG-640-OPEN.md)** — он снимает
реальные вехи и расширяет `deliver_nav_timing` до структуры. Пока
источника нет, этот баг чинить нечем.

После него:

1. Интерфейсы `PerformanceTiming` и `PerformanceNavigation` с
   собственными `toJSON()` (оба объявлены в IDL как `[Default] object
   toJSON()`), атрибуты `performance.timing` / `performance.navigation`
   — getter-only, как весь остальной `Performance` после BUG-400.
2. Значения брать **из того же снимка**, что и L2-запись BUG-640, а не
   вторым каналом: L1 и L2 описывают одну навигацию в разных единицах
   (L1 — unix-epoch мс, L2 — `DOMHighResTimeStamp` от `timeOrigin`), и
   два независимых источника заведут ровно то расхождение половин, от
   которого предостерегает
   [ADR-026](../docs/decisions/ADR-026-global-privacy-control-signal.md).
   Конверсия L2→L1 — одна функция, а не копия сбора.
3. Добавить `timing`/`navigation` в `Performance.prototype.toJSON` —
   сейчас там только `timeOrigin`, с комментарием, ссылающимся на этот
   баг.
4. `PerformanceNavigation.type` (`TYPE_NAVIGATE`/`TYPE_RELOAD`/
   `TYPE_BACK_FORWARD`) и `redirectCount` — те же значения, что BUG-640
   требует для `type`/`redirectCount` L2-записи, только в числовой
   легаси-форме; отдельного источника не заводить.

## Связанные

* [BUG-640](BUG-640-OPEN.md) — **блокирует этот баг**: тот же корень
  (`deliver_nav_timing` без данных), другая поверхность (L2-запись).
* [BUG-400](BUG-400-FIXED.md) — `Performance` как интерфейс + `toJSON()`;
  заведён этим фиксом, там же объяснено, почему `timing`/`navigation`
  вынесены сюда.
* [BUG-520](BUG-520-OPEN.md) — `_lumen_record_resource_timing` не
  вызывается ни из одного реального пути загрузки: соседний случай
  «JS-сторона есть, данных нет», но по Resource Timing.
* `CAPABILITIES.md` строка «Observers/Timing» — Navigation Timing уже 🟡
  по BUG-640; отдельной пометки этот баг не требует.
