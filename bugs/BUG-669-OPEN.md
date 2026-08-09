# BUG-669 — `WakeLock` interface object never exposed on `globalThis` (only `navigator.wakeLock` and `WakeLockSentinel` are)

**Статус:** OPEN
**Компонент:** js (`crates/js/src/wake_lock.rs`, `WAKE_LOCK_SHIM` — Screen Wake Lock Level 1 Phase 1 shim)
**Найден:** P2, WPT-VENDOR-screen-wake-lock, 2026-08-06

## Симптом

Категория `screen-wake-lock` (`tests/wpt/screen-wake-lock/`, 21 файл) — вендорена и
прогнана целиком (`run_report.py --all --root screen-wake-lock --recursive`, ~4:39,
16 отобранных id): **2/16 harness OK, 1/2 сабтестов**. 13 из 16 id — `.https.`,
TIMEOUT на уже задокументированном TLS-гэпе `UnknownIssuer`; 2 ERROR — тот же
session/URL-reuse артефакт, что и в других категориях этого бэклога (реальный
результат прилетает под URL предыдущего теста, `assert result_url == test.url`
в `base.py:104` ловит несовпадение, не даёт ложных зелёных). Из двух реально
исполнившихся не-`.https.` тестов один (`wakelock-supported-by-permissions-policy.html`)
— реконфирмация уже открытого [BUG-361](BUG-361-FIXED.md)
(`document.permissionsPolicy.features()` всегда `[]`); второй,
`wakelock-insecure-context.any.html`, формально **PASS**:

```js
//META: title=Wake Lock API is not exposed in an insecure context
test(() => {
  assert_false("WakeLock" in self, "'WakeLock' must not be exposed");
}, "Wake Lock API is not exposed in an insecure context");
```

Тест проходит по неверной причине: живая проба (`--mcp-live-port`, страница
на плейн-HTTP `http://127.0.0.1:18999/…`) показывает, что `'WakeLock' in self`
даёт `false` **всегда**, независимо от контекста:

```json
{"isSecureContext":true,"hasWakeLockGlobal":false,"wakeLockInSelf":false,
 "navWakeLock":"object","navWakeLockRequest":"function"}
```

(`isSecureContext` здесь `true` — реконфирмация отдельного, уже открытого
[BUG-399](BUG-399-OPEN.md), «`window.isSecureContext` захардкожен `true`»; то
есть эта страница по мнению движка — secure context, а `WakeLock` там всё равно
отсутствует.) `navigator.wakeLock.request('screen')` при этом реально
работает и резолвится (`{"ok":true,"type":"screen","released":false}`) — сам
API функционален, дело не в отсутствии Wake Lock вообще.

## Причина

`WAKE_LOCK_SHIM` (`crates/js/src/wake_lock.rs:92`–`214`) реализует
`WakeLockSentinel` и внутренний менеджер `_WakeLock`, экспортируя:

```js
globalThis.WakeLockSentinel = WakeLockSentinel;
...
Object.defineProperty(navigator, 'wakeLock', { get: function() { return _WakeLock; } });
```

Спека (W3C Screen Wake Lock API §4, `wakelock.idl`) описывает третий,
отдельный интерфейс:

```webidl
[Exposed=(Window,Worker), SecureContext]
interface WakeLock {
  Promise<WakeLockSentinel> request(optional WakeLockType type = "screen");
};
```

— `WakeLock` сам по себе обязан существовать как именованный глобальный
объект (интерфейс, на который ссылается `navigator.wakeLock`'s `[SameObject]`
атрибут). Шим никогда не присваивает `globalThis.WakeLock` ни при каком
условии — только `WakeLockSentinel`. Отсюда `'WakeLock' in self` возвращает
`false` безусловно, в любом контексте, включая тот, что сам движок считает
secure (`isSecureContext === true`).

## Масштаб

Единственная новая находка категории — оба других сигнала уже покрыты
открытыми тикетами (BUG-361/BUG-399). `wakelock-insecure-context.any.html`
проходит только потому, что интерфейса нет нигде — тот же тест на реальном
secure-context (`.https.`) окружении, если бы TLS-гэп не блокировал 13/13
`.https.`-файлов категории, обязан был бы провалиться на любом позитивном
чеке присутствия `WakeLock` (`idlharness.https.window.html` в этой же
категории проверяет именно это через WebIDL-интроспекцию — заблокирован
TLS-гэпом целиком в этом прогоне, но зафиксировал бы то же самое напрямую).

## Дальше

Fix scope: добавить в `WAKE_LOCK_SHIM` минимальный конструктор-интерфейс
`function WakeLock() {}` (не вызываемый напрямую пользовательским кодом — сам
`request()` уже реализован на `navigator.wakeLock`, здесь нужен только сам
факт присутствия интерфейса на глобальном объекте) и
`globalThis.WakeLock = WakeLock;`, аналогично тому, как уже сделано для
`WakeLockSentinel`. Вне скоупа этой WPT-VENDOR-задачи (только вендоринг +
прогон + живая проба).
