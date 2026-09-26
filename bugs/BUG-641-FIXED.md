# BUG-641: `navigator.connection.downlinkMax` missing from the `NetworkInformation` stub

**Статус:** FIXED 2026-09-25 (P3)
**Компонент:** js (`crates/js/src/dom.rs`, Network Information API shim)
**Найден:** P2, WPT-VENDOR-netinfo, 2026-08-05

## Симптом

`netinfo` (скоуп ⬜, кандидат) — вендорена и прогнана целиком
(`run_report.py --all --root netinfo --recursive`, ~30 с, 2 отобранных
id): **1/2 harness OK, 5/6 сабтестов**.

`netinfo-basics.html` (harness OK) — единственный провал:

```
FAIL downlinkMax attribute - assert_greater_than_equal: expected a number but got a "undefined"
```

## Причина

`crates/js/src/dom.rs:12828-12847` — комментарий на месте прямо называет
реализацию заглушкой:

```js
// ── Network Information API (W3C Network Information §7) ──────────────────────
// navigator.connection — effective type, downlink, rtt, saveData.
// Phase 1 stub: reports '4g'/10 Mbps/100 ms (reasonable desktop default).
(function() {
  function NetworkInformation() {
    this.effectiveType = '4g';
    this.downlink      = 10;
    this.rtt           = 100;
    this.saveData      = false;
    this.type          = 'wifi';
    this._onchange     = null;
  }
  ...
```

`downlinkMax` — обязательный атрибут интерфейса `NetworkInformation` по
черновику WICG Network Information API
(https://wicg.github.io/netinfo/#dom-networkinformation-downlinkmax,
`readonly attribute double downlinkMax;`) — единственное поле, не
перенесённое в шим вместе с `effectiveType`/`downlink`/`rtt`/`saveData`/
`type`. Не WebIDL-freeze/enumerability дефект (тот класс уже заведён как
BUG-636 на соседнем `mediasession`) — атрибут просто отсутствует.

## Масштаб

Единственный сабтест-провал категории. Второй id (`idlharness.any.html`)
не даёт сигнала — TIMEOUT на уже задокументированном невендоренном
`/resources/idlharness.js`/`WebIDLParser.js` (тот же класс, что
`tests/wpt/VENDOR.md` уже фиксирует для `FileAPI`/`animation-worklet` —
только `testharness.js`+`testharnessreport.js` вендорены под
`tests/wpt/resources/`), не новая находка.

## Дальше

Fix scope: добавить `this.downlinkMax = Infinity;` (или конечное
разумное значение по духу существующих дефолтов, спека не требует
конкретики) в конструктор `NetworkInformation` рядом с остальными
полями-заглушками. Тривиальный однострочный фикс, но вне скоупа этой
WPT-VENDOR-задачи (только вендоринг + прогон).

## Исправление (P3, 2026-09-25)

Шим переехал из `dom.rs` в `crates/js/src/shim/web_api_shim_tail_b.js`
(блок «Network Information API»). Конструктор `NetworkInformation` теперь
ставит `downlinkMax = Infinity` — по WICG Network Information
(«max downlink speed»): UA без знания о максимальной скорости канала
отдаёт `+Infinity`. Конечное число выдумывать не стали — `type = 'wifi'`
у заглушки такой же условный, как и остальные поля, а `+Infinity` —
единственное значение, которое спека прямо называет для «не знаю».

Регрессионный тест: `navigator_connection_downlink_max_is_unknown_infinity`
(`crates/js/src/dom/tests/v8_fullscreen_locks.rs`). Метаданные
`tests/wpt/metadata/netinfo/netinfo-basics.html.ini` (единственная запись —
`expected: FAIL` для этого сабтеста) удалены.

Живой прогон `run_report.py --all --root netinfo --recursive`:
`netinfo-basics.html` — **6/6**, Unexpected 0. `idlharness.any.html` теперь
не TIMEOUT (idlharness.js вендорен с тех пор) и падает на WebIDL-форме
интерфейса (атрибуты — собственные свойства экземпляра, а не аксессоры
прототипа; `NetworkInformation` enumerable на `window`) — это класс
[BUG-664](BUG-664-FIXED.md), не эта заявка.
