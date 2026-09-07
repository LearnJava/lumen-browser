# BUG-640: `PerformanceNavigationTiming` entry is a bare stub — only `entryType`/`name`/`startTime`/`duration` are populated, every other spec field is `undefined`

**Статус:** FIXED 2026-09-07
**Компонент:** shell + js (`crates/shell/src/main.rs::deliver_nav_timing`, `crates/js/src/dom.rs::_lumen_deliver_perf_entry`) — на момент фикса
перенесено SPLIT-треком в `crates/shell/src/persistent_js.rs::deliver_nav_timing` и
`crates/js/src/shim/web_api_shim_tail.js::_lumen_deliver_perf_entry`
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

## Фикс (2026-09-07, P3)

Новый модуль `crates/shell/src/nav_timing.rs` строит полноценный
`detail_json` для `deliver_nav_timing` — все 30 атрибутов, которые
`nav2-test-attributes-exist.html` проверяет через `in`, плюс
`responseStatus`/`initiatorType`/`serverTiming`. Реальные значения там,
где движок реально что-то измеряет; честный `0`/`""`/`[]` там, где нет
(см. doc-комментарий модуля — секции «real» и «honestly stubbed»
перечисляют каждое поле явно, вместо того чтобы подставить правдоподобно
выглядящие числа):

- **`responseStatus`/`encodedBodySize`/`decodedBodySize`/`transferSize`** —
  реальные. `lumen_network::PageResponse` получил поле `status: u16`
  (шесть точек конструирования в `fetch_page`/`fetch_page_streaming`,
  `crates/network/src/lib.rs`) — раньше `resp.status` терялся до
  `PageResponse`, хотя был доступен на каждом call site. Протянуто через
  `RawPage` (`page_source.rs`) → `render_bytes` (два новых параметра
  `response_status`/`redirected`) → `LoadedPage::nav`
  (`NavResponseMeta { status, redirected, decoded_body_size }`) — читается
  на обоих реальных call site `deliver_nav_timing` (`page_load.rs`
  синхронный fallback, `app/user_event.rs` стриминговый путь) **до**
  перемещения `page` в `apply_loaded_page`, так как `nav` — единственное
  поле, которое ещё нужно после этого момента.
- **`redirectCount`** — `0`/`1` (не точный счётчик хопов: у
  `fetch_with_redirect`'s `hops_left` — обратный отсчёт, не счётчик,
  наружу не отдаётся; менять сигнатуру рекурсивной 33-параметровой
  функции ради одного WPT-сабтеста признано несоразмерным риском).
  Сравнение `final_url != lumen_url` в `page_source.rs` — честный нижний
  предел, а не выдуманная точность.
- **DOM-вехи** (`domInteractive`/`domContentLoadedEventStart/End`/
  `domComplete`/`loadEventStart/End`) — реальные `Instant`, относительно
  того же `nav_start`, что и `duration_ms` (не JS `performance.now()` —
  `timeOrigin` фиксируется в `install_dom`, строго позже `nav_start`,
  смешивание часов сломало бы монотонность энтри). Захватываются вокруг
  `notify_dom_content_loaded()` (`page_pipeline.rs`, фоновый поток) и
  `notify_window_loaded()` (`page_load.rs::apply_loaded_page`, через
  `route_task_js`) в процесс-глобальную таблицу меток — тот же паттерн,
  что `resource_timing.rs`'s process-global очередь, по той же причине
  (кросс-поточность, `notify_*` не имеет доступа к состоянию, которое
  видит `deliver_nav_timing`'s call site).
- **Честно застабленные `0`** (не измеряются нигде в движке):
  `redirectStart`/`redirectEnd` (нет пер-хоповых таймстемпов),
  `domainLookupStart/End`/`connectStart/End`/`secureConnectionStart`/
  `requestStart`/`fetchStart`/`responseStart`/`responseEnd` (у
  `lumen-network` нет разбивки DNS/connect/TLS/request на фазы — тот же
  гэп, что `_lumen_record_resource_timing` уже документирует для
  подресурсов), `unloadEventStart/End` (предыдущий документ не
  таймируется), `workerStart`, `activationStart` (нет prerendering).
- **`type`** — всегда `"navigate"`. Все виды навигации (`navigate_to`/
  `navigate_replace`/`navigate_back`/`navigate_forward`/реальный reload)
  сходятся в единственной точке `Lumen::reload()` без метки, кто её
  вызвал; протяжка `pending_nav_type` через 11 call site (несколько с
  ранними `return` при перехваченной навигации) ради одного непрогоняемого
  сабтеста (`nav2-test-navigation-type-navigate.html`, который как раз и
  проверяет обычную навигацию — то есть корректный дефолт) оставлена как
  документированный остаток, не сделана вслепую.
- **`serverTiming`/`nextHopProtocol`** — `[]`/`""`, `Server-Timing` заголовок
  нигде не парсится (не в скоупе).

7 новых юнит-тестов в `nav_timing.rs` (`cargo test -p lumen-shell --bin
lumen -- nav_timing`), в т.ч. проверка, что все 26
специфичных-для-navigation атрибутов присутствуют в JSON, что
`transferSize` обнуляется на кэш-хите, и что DOM-вехи растут в правильном
порядке. `cargo check --workspace --all-targets` чист;
`cargo clippy -p lumen-network -p lumen-shell --all-targets -- -D
warnings` не прогнать целиком на этой машине (локальный `rustc`/clippy
1.98.0 против пина 1.97.0 красит несвязанные файлы — `chunks_exact` в
`lumen-image`/`dump_mode.rs`/h2/h3/websocket, ни один не задет этим
диффом, `git diff --stat` подтверждает).

**Остаток (не в этом фиксе):** HTTP `Content-Type`-заголовок для навигации
не хранится нигде дальше `lumen-network`'s внутреннего `Response` — тот же
класс гэпа, что BUG-509 оставил для CSS-стилшитов, здесь того же решения
не потребовалось, потому что `responseStatus` не зависит от заголовков.
Точный `redirectCount` и различение `type` — задокументированы выше как
сознательно не сделанные, не забытые. [BUG-767](BUG-767-FIXED.md)
(`performance.timing`/`performance.navigation`, устаревший L1-интерфейс)
разблокирован этим фиксом и может брать значения из того же `nav_timing`
модуля вместо второго независимого источника данных.
