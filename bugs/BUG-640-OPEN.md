# BUG-640: `PerformanceNavigationTiming` entry is a bare stub — only `entryType`/`name`/`startTime`/`duration` are populated, every other spec field is `undefined`

**Статус:** OPEN
**Компонент:** shell + js (`crates/shell/src/main.rs::deliver_nav_timing`, `crates/js/src/dom.rs::_lumen_deliver_perf_entry`)
**Найден:** P2, WPT-VENDOR-navigation-timing, 2026-08-05

## Симптом

`navigation-timing` (скоуп ⬜, кандидат) — вендорена и прогнана целиком
(`run_report.py --all --root navigation-timing --recursive`, ~8 мин, 59
отобранных id): **12/58 harness OK, 4/36 сабтестов (11%)**.

Среди тестов, где harness завершился (не TIMEOUT/ERROR), почти каждый
провал — обращение к атрибуту `PerformanceNavigationTiming`, который
оказывается `undefined`:

- `nav2-test-attributes-exist.html`: `assert_true: Expected attribute:
  connectEnd. expected true got false`
- `nav2-test-instance-accessible-from-the-start.html`:
  `assert_not_equals: got disallowed value undefined` (сам объект entry
  почти пуст)
- `nav2-test-navigation-type-navigate.html`: `assert_equals: Expected
  navigation type to be navigate. expected (string) "navigate" but got
  (undefined) undefined`
- `nav2-test-redirect-none.html`: `assert_equals: Expected redirectCount
  to be 0. expected (number) 0 but got (undefined) undefined`
- `test-document-onload.html` (2 сабтеста): `Cannot read properties of
  undefined (reading 'transferSize')`

Чтение исходника (не догадка по логу) подтверждает причину:
`crates/shell/src/main.rs:3231-3236`:

```rust
fn deliver_nav_timing(&self, url: &str, duration_ms: f64) {
    self.eval_js(&format!(
        "_lumen_deliver_perf_entry('navigation', {}, 0.0, {duration_ms}, null)",
        js_string_literal(url),
    ));
}
```

`detail_json` — четвёртый аргумент `_lumen_deliver_perf_entry`
(`crates/js/src/dom.rs:8417-8434`) — передаётся как литеральный `null`,
поэтому цикл `for (var k in extra)`, который должен домешать
дополнительные поля в entry, никогда не выполняется. Итоговый объект
`entry` содержит **только** `entryType`, `name`, `startTime`, `duration`
— ни одного из полей `PerformanceNavigationTiming` (W3C Navigation
Timing L2 §4.2): `connectStart/End`, `domainLookupStart/End`,
`secureConnectionStart`, `requestStart`, `responseStart/End`,
`redirectStart/End`, `redirectCount`, `type`, `unloadEventStart/End`,
`domInteractive`, `domContentLoadedEventStart/End`, `domComplete`,
`loadEventStart/End`, `transferSize`, `encodedBodySize`,
`decodedBodySize`, `responseStatus`, `activationStart`, `serverTiming`.

Для сравнения, соседний резолвер `PerformanceResourceTiming`
(`_lumen_record_resource_timing`, строки 8385-8412) строит полноценный
объект со всеми под-таймингами — тот же паттерн просто не был перенесён
на `navigation`-запись при добавлении `deliver_nav_timing`.

## Причина

`deliver_nav_timing` — единственный производитель `entryType: 'navigation'`
записей (кроме юнит-тестов в самом `dom.rs`, строки 14324+, которые тоже
не передают detail) — никогда не заполнял `detail_json`, поэтому вся
экосистема `PerformanceNavigationTiming` в Lumen — заглушка из четырёх
базовых полей `PerformanceEntry`, унаследованных от родительского
интерфейса, без единого специфичного для `navigation` атрибута.

## Масштаб

Доминирующая причина всех непройденных сабтестов на тестах с harness OK
(5 из 6 файлов, где harness завершился без TIMEOUT/ERROR). Не
единственная причина категории — остальные 46/58 TIMEOUT/ERROR
распадаются на два уже известных, не связанных с этим багом класса:

- **TLS `UnknownIssuer`** (`docs/wpt-status.md:25-28`) — все `.https.`
  тесты (`secure-connection-start-non-zero.https.html`,
  `secure-connection-start-reuse.https.html`,
  `response-start-after-coop-bcg-switch.https.html`).
- **Cross-origin/iframe/xserver-редирект навигация** (класс
  `BUG-480`-типа, нет полноценного multi-window/iframe browsing
  context для навигационных тестов) — вся серия `nav2-test-redirect-*`,
  `nav2-test-navigate-iframe.html`, `test-timing-*redirect*.html` и
  аналогичные, зависшие на TIMEOUT в ожидании второго origin/окна.

Три файла (`nested-unload-timing.html`, `prefetch-transfer-size-
executor.html`, `redirect-tao.html`) используют `RemoteContext` из
`/common/dispatcher/dispatcher.js` — категория-внешняя зависимость,
которая **не вендорена** (в репозитории нет `tests/wpt/common/`
вообще) — тот же паттерн, что уже отмечен в
`WPT-VENDOR-mixed-content`/`WPT-VENDOR-mst-content-hint`
(`/common/security-features/...`, `/webrtc/RTCPeerConnection-helper.js`).
Не заводился как отдельный баг — известный класс "категория-внешняя
зависимость вне скоупа вендоринга этой задачи".

## Дальше

Fix scope: заменить `null` в `deliver_nav_timing` на JSON-объект с
реальными таймингами (аналогично `_lumen_record_resource_timing`) —
нужны данные от `crates/shell` о фазах загрузки (redirect count/timing,
DNS/connect/TLS фазы, `domInteractive`/`domContentLoaded`/`load` события,
`type` — 'navigate'/'reload'/'back_forward'/'prerender'). Часть данных
(DOM-события) уже наверняка доступна в шелле (используется для
`readystatechange`); часть (redirect chain, network sub-timings) может
требовать протяжки через `lumen-network`, аналогично тому, как это уже
сделано для `PerformanceResourceTiming`.
