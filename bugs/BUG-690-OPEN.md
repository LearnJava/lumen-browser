# BUG-690 — `NavigatorUAData` is directly constructible with `new` and lacks `Symbol.toStringTag`

**Статус:** OPEN
**Компонент:** js (`crates/js/src/ua_client_hints.rs` — `UA_CLIENT_HINTS_SHIM`)
**Найден:** P2, WPT-VENDOR-ua-client-hints, 2026-08-09

## Симптом

Категория `ua-client-hints` (`tests/wpt/ua-client-hints/`, 2 отобранных id из 4
файлов — `META.yml`/`WEB_FEATURES.yml` не тесты) — вендорена и прогнана целиком
(`run_report.py --all --root ua-client-hints --recursive`, ~1 мин): **0/2
harness OK**. Оба id — `.https.` (`idlharness.https.any.js`,
`useragentdata.https.any.js`), оба TIMEOUT на уже задокументированном TLS-гэпе
`UnknownIssuer` (`network error: TLS handshake: invalid peer certificate:
UnknownIssuer`), ни один не дошёл до навигации.

`navigator.userAgentData` реально реализован (Phase 0, статический профиль
Chrome 114/Windows 10 — `ua_client_hints.rs`), и по функциональной части
выглядит корректно: `brands`/`mobile`/`platform` — геттеры на прототипе (не
собственные enumerable-свойства инстанса), `getHighEntropyValues()` резолвится
Promise'ом с нужными полями, `toJSON()` отдаёт корректную структуру — все
ассерты `useragentdata.https.any.js` прошли бы, если бы тест исполнился.
Живая проба (`--mcp-live-port`) нашла два независимых WebIDL-дефекта формы,
не связанных с TLS-гэпом:

```json
"Object.prototype.toString.call(navigator.userAgentData)" -> "[object Object]"
"new NavigatorUAData() throws?" -> "NO THROW"
```

1. Спека (WICG UA-CH §4) не определяет операцию-конструктор для
   `NavigatorUAData` — объект существует только как единственное значение
   `navigator.userAgentData`, выданное движком. Но `UA_CLIENT_HINTS_SHIM`
   объявляет его обычной `function NavigatorUAData() {}` без
   `new.target`-guard, поэтому `new NavigatorUAData()` со страницы успешно
   конструирует объект, неотличимый через `instanceof NavigatorUAData` от
   настоящего.
2. `Object.prototype.toString.call(navigator.userAgentData)` возвращает
   `"[object Object]"` вместо `"[object NavigatorUAData]"` — на прототипе нет
   `[Symbol.toStringTag]`, что `idlharness.https.any.js` (не исполнился из-за
   TLS-гэпа, но проверяет это в общем WebIDL-обвязке) считает нарушением.

## Масштаб

Тот же класс дефекта, что уже открыт для `SerialPort`/`Serial`
([BUG-672](BUG-672-OPEN.md)), `Report`/`ReportingObserver`
([BUG-629](BUG-629-OPEN.md)), `FileSystemFileHandle`
([BUG-374](BUG-374-FIXED.md)) и `FaceDetector`/`BarcodeDetector`/`TextDetector`
([BUG-677](BUG-677-OPEN.md)) — интерфейс без спекового конструктора,
подделываемый со страницы; здесь дополнительно совпадает и с отсутствующим
`Symbol.toStringTag`, второй частью того же BUG-677. Функциональный WPT-сигнал
по категории отсутствует целиком (TLS-гэп режет оба id до навигации),
находка — только из живой пробы.

## Причина

Не установлена детально (вне скоупа WPT-VENDOR-задачи).
`UA_CLIENT_HINTS_SHIM` (`crates/js/src/ua_client_hints.rs:57`) объявляет
`function NavigatorUAData() {}`, вызываемую и через `new NavigatorUAData()` со
страницы, и внутри самого шима как `new NavigatorUAData()` для создания
единственного экземпляра — общая конструкторная функция без разделения
"внутренняя фабрика" / "публичный вызов" и без `[Symbol.toStringTag]` на
`NavigatorUAData.prototype`.

## Дальше

Fix scope: заблокировать публичный `new NavigatorUAData()` (guard через
непубличный токен, передаваемый из внутреннего вызова, либо throw в
конструкторе + отдельная фабричная функция для единственного инстанса,
устанавливаемого в `navigator.userAgentData`) и добавить `Object.defineProperty
(NavigatorUAData.prototype, Symbol.toStringTag, {value: 'NavigatorUAData',
configurable: true})`. Не требует TLS-гэпа для воспроизведения/фикса — живой
`--mcp-live-port`-пробы достаточно для верификации.
