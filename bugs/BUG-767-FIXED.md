# BUG-767 — `performance.timing` / `performance.navigation` (legacy Navigation Timing L1) отсутствуют

**Статус:** FIXED 2026-09-07 (P3)
**Компонент:** js (`crates/js/src/shim/performance_shim.js` — `Performance.prototype`)
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

`performance.timing` и `performance.navigation` были `undefined`. Грепом
по `crates/` имена `PerformanceTiming`/`PerformanceNavigation` не
встречались ни разу.

## Причина

Legacy-интерфейсы Navigation Timing L2 §5–6 (`PerformanceTiming`,
`PerformanceNavigation`, выставляемые partial-атрибутами `Performance`)
не были реализованы вовсе. Формально это отдельная от
[BUG-640](BUG-640-FIXED.md) поверхность — там речь о современной
`PerformanceNavigationTiming`-записи (L2, `getEntriesByType('navigation')`),
здесь о двух легаси-атрибутах самого `performance`, — но корень у них
общий: до BUG-640 снимков вех навигации в движке не было ни одного;
после его фикса (2026-09-07) реальный снимок есть —
`crates/shell/src/nav_timing.rs`.

## Исправление

`PerformanceTiming`/`PerformanceNavigation` (`crates/js/src/shim/performance_shim.js`)
конструируются из того же снимка, что и L2-запись BUG-640
(`_perf_last_navigation_entry`, обновляемый `_lumen_deliver_perf_entry` в
`web_api_shim_tail.js`) — одна функция конверсии (`_perf_l2_to_l1`), а не
второй канал данных, как предостерегает [ADR-026](../docs/decisions/ADR-026-global-privacy-control-signal.md).

- `performance.timing` — getter, возвращающий `PerformanceTiming` с 21
  вехой в спековом порядке; `navigationStart`/`domLoading` — Unix-epoch
  мс от `timeOrigin` (L2 хранит `DOMHighResTimeStamp`-и, L1 — абсолютные
  метки, конверсия — `_perf_l2_to_l1`); всё, что ещё не произошло или
  честно застаблено нулём в L2-снимке (DNS/connect/TLS-фазы,
  redirect-таймстемпы, `unloadEvent*`), даёт спековый фолбэк `0`.
  `domLoading` удалён из L2 (нет соответствующего поля у снимка) —
  оставлен L1-олдскульным алиасом `navigationStart`, а не выдуманной
  промежуточной меткой.
- `performance.navigation` — getter, возвращающий `PerformanceNavigation`
  с `type` (числовая легаси-константа `TYPE_NAVIGATE`/`TYPE_RELOAD`/
  `TYPE_BACK_FORWARD`/`TYPE_RESERVED`, смаппленная из той же L2-строки
  `nav_timing.rs` уже даёт) и `redirectCount` (то же `0`/`1`, что и L2).
- Оба интерфейса — `[Illegal constructor]`, с собственным `toJSON()` (IDL
  `[Default] object toJSON()`), симметрично `Performance`/`PerformanceEntry`.
- `Performance.prototype.toJSON` дополнен `timing`/`navigation` (раньше
  — только `timeOrigin`), через их же `toJSON()`.

6 новых юнит-тестов в `dom/tests/v8_perf_typedom_node.rs`. Два
существовавших теста обновлены под новую (спеково верную) форму
`toJSON()`: `performance_to_json_carries_attributes_only`
(`v8_perf_observers.rs`) и `v8_worker_globals_have_performance`
(`worker.rs`) — оба раньше жёстко фиксировали `toJSON()` на одном
`timeOrigin`. `cargo test -p lumen-js --features v8-backend performance`
— 43/43 зелёных. `cargo check --workspace --all-targets` чист;
`cargo clippy` целиком не прогнать на этой машине — локальный
rustc/clippy 1.98.1 против пина 1.97.0 красит несвязанные файлы
(`chunks_exact` в `lumen-image`), подтверждено `git diff --stat`.

## Связанные

* [BUG-640](BUG-640-FIXED.md) — был блокером, исправлен 2026-09-07: тот
  же снимок вех навигации, другая поверхность (L2-запись).
* [BUG-400](BUG-400-FIXED.md) — `Performance` как интерфейс + `toJSON()`;
  заведён этим фиксом, там же объяснено, почему `timing`/`navigation`
  были вынесены сюда.
