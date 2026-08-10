# BUG-713 — `HIDManager`/`HIDDevice` are directly constructible with `new`, though the spec defines no constructor

**Статус:** OPEN
**Компонент:** js (`crates/js/src/webhid.rs` — `WEBHID_SHIM`)
**Найден:** P2, WPT-VENDOR-webhid, 2026-08-09

## Симптом

Категория `webhid` (`tests/wpt/webhid/`, 11 файлов) — вендорена и прогнана
целиком (`run_report.py --all --root webhid --recursive`, ~1:35, 5 отобранных
id — все `.https.`): **0/5 harness OK**. Все пять TIMEOUT на уже
задокументированном TLS-гэпе `UnknownIssuer`, ни один не дошёл до навигации —
не новая находка (тот же TLS-гэп, что режет `webauthn`/`serial`/`webgl`).

WebHID API реально реализован как Phase 0 заглушка
(`crates/js/src/webhid.rs`, `navigator.hid.requestDevice()` намеренно бросает
`NotSupportedError`, `getDevices()` резолвится в `[]` — соответствует
doc-комментарию в исходнике). Живая проба (`--mcp-live-port`) нашла
независимый дефект в WebIDL-форме обоих интерфейсов:

```json
{
  "HIDManager_type": "function",
  "HIDDevice_type": "function",
  "new_HIDManager_ok": true,
  "new_HIDDevice_ok": true,
  "hid_instanceof_EventTarget": true
}
```

Оба интерфейса (`window.HIDManager`, `window.HIDDevice`) реализованы как
обычные ES6 `class X extends EventTarget`, поэтому `new HIDManager()`/
`new HIDDevice(1, 2, 'x')` успешно конструируются с любой (или без) страницы,
хотя апстримная спека (WICG WebHID) не определяет конструктор ни для `HID`
(`navigator.hid` — синглтон, единственный экземпляр), ни для `HIDDevice`
(элементы, которые может создавать только сам движок — результат
`getDevices()`/`requestDevice()`). По WebIDL интерфейс без
операции-конструктора при вызове с `new` обязан бросать `TypeError`.
(`HIDConnectionEvent`, третий интерфейс модуля, спекой конструктор *имеет*
— не проверялся отдельно, вне подозрения.)

## Масштаб

Тот же класс дефекта, что уже открыт для `Report`/`ReportingObserver`
([BUG-629](BUG-629-OPEN.md)), `FileSystemFileHandle`
([BUG-374](BUG-374-FIXED.md)) и `Serial`/`SerialPort`
([BUG-672](BUG-672-OPEN.md)) — подделываемый объект, неотличимый через
`instanceof HIDManager`/`instanceof HIDDevice` от настоящего, выданного
движком. Здесь — четвёртая независимая поверхность того же системного
паттерна (`WEBHID_SHIM` не ставит guard на `new.target`/не блокирует
конструктор). Функциональный WPT-сигнал по категории отсутствует целиком
(TLS-гэп режет все 5 id до навигации), находка — только из живой пробы.

Отдельно (не новый баг, уже покрыто [BUG-615](BUG-615-OPEN.md), который явно
называет `webhid.rs` в списке затронутых модулей): `navigator.hid` продолжает
утечку `_devices`/`onconnect`/`ondisconnect` как собственные перечислимые
свойства (`Object.getOwnPropertyNames(navigator.hid)` →
`["_listeners", "onconnect", "ondisconnect", "_devices"]`).

## Причина

Не установлена детально (вне скоупа WPT-VENDOR-задачи). `WEBHID_SHIM`
(`crates/js/src/webhid.rs:19-114`) объявляет `class HIDDevice extends
EventTarget { constructor(vendorId, productId, productName, collections) {
... } }` и `class HIDManager extends EventTarget { constructor() { ... } }`
без проверки, что вызов пришёл из движка, а не со страницы — обычный JS
`class` конструктор общедоступен по определению, нужен явный guard
(непубличный символ, передаваемый из фабрики движка, либо throw + внутренняя
фабричная функция), которого здесь нет. Идентичная структура коду
`serial.rs`, откуда и унаследован дефект (общий S5-S7 V8-порт эпохи Ph3).

## Дальше

Fix scope: заблокировать публичный `new HIDManager()`/`new HIDDevice(...)`
(тот же guard-паттерн, что предложен для [BUG-672](BUG-672-OPEN.md); имеет
смысл чинить оба файла вместе — общий источник дефекта). Не требует
TLS-гэпа для воспроизведения/фикса — живой `--mcp-live-port`-пробы
достаточно для верификации.
