# BUG-681 — `StorageManager`/`StorageBucket`/`StorageBucketManager` are directly constructible with `new`, though neither spec defines a constructor

**Статус:** OPEN
**Компонент:** js (`crates/js/src/storage_manager.rs` — `STORAGE_MANAGER_SHIM`; `crates/js/src/storage_buckets.rs` — `STORAGE_BUCKETS_SHIM`)
**Найден:** P2, WPT-VENDOR-storage, 2026-08-06

## Симптом

Категория `storage` (`tests/wpt/storage/`, 32 значимых файла) — вендорена и
прогнана целиком (`run_report.py --all --root storage --recursive`, 7:47, 26
отобранных id, `persist-permission-manual.https.html` исключён собственным
фильтром раннера): **0/26 harness OK**. Все 26 — `.https.`, все TIMEOUT на уже
задокументированном TLS-гэпе `UnknownIssuer` (`network error: TLS handshake:
invalid peer certificate: UnknownIssuer`), ни один не дошёл до навигации.

`navigator.storage` (`StorageManager`) и `navigator.storageBuckets`
(`StorageBucketManager`/`StorageBucket`) реально реализованы как Phase 0
in-memory заглушки, задокументированные собственными doc-комментариями
(`estimate()` → 0 байт/10 GiB, `persist()`/`persisted()` → `true`, бакеты
живут только в памяти контекста). Живая проба (`--mcp-live-port`) на форме
всех трёх интерфейсов нашла независимый WebIDL-дефект:

```json
"new StorageManager() succeeds?"       -> "constructed-ok" (typeof .estimate === 'function')
"new StorageBucketManager() succeeds?" -> "constructed-ok" (typeof .open === 'function')
"new StorageBucket('x', {}) succeeds?" -> "constructed-ok" (.name === 'x')
```

Все три (`window.StorageManager`, `window.StorageBucketManager`,
`window.StorageBucket`) реализованы как обычные ES5 `function X() { ... }` без
проверки `new.target`, поэтому конструируются напрямую с любой страницы —
хотя ни WHATWG Storage Standard §9, ни W3C Storage Buckets не определяют
конструктор ни для одного из трёх интерфейсов (`StorageManager` доступен
только как единственный синглтон `navigator.storage`; `StorageBucketManager`
— только как синглтон `navigator.storageBuckets`; `StorageBucket` — только
как значение, отдаваемое `storageBuckets.open()`). По WebIDL интерфейс без
операции-конструктора при вызове с `new` обязан бросать `TypeError`.

`navigator.permissions.query({name:'persistent-storage'})` также резолвился
`granted` (реконфирмация [BUG-386](BUG-386-FIXED.md), который прямо перечисляет
`persistent-storage` как один из непроверяемых нереализованных имён), не новая
находка. **Устарело с 2026-08-10:** BUG-386 закрыт, `persistent-storage`
отвечает `denied` — `persist()`/`persisted()` по-прежнему резолвятся `true`, ни
на что не влияя, и это расхождение теперь видно.

## Масштаб

Тот же класс дефекта, что уже открыт для `Report`/`ReportingObserver`
([BUG-629](BUG-629-OPEN.md)), `FileSystemFileHandle`
([BUG-374](BUG-374-FIXED.md)) и `Serial`/`SerialPort`
([BUG-672](BUG-672-OPEN.md)) — подделываемый объект, неотличимый через
`instanceof` от настоящего, выданного движком. Здесь — четвёртая-шестая
независимая поверхность того же системного паттерна (ни один из
`storage_manager.rs`/`storage_buckets.rs` шимов не ставит guard на
`new.target`/не блокирует публичный конструктор), затрагивает сразу три
интерфейса в двух модулях. Функциональный WPT-сигнал по категории отсутствует
целиком (TLS-гэп режет все 26 id до навигации), находка — только из живой
пробы.

## Причина

Не установлена детально (вне скоупа WPT-VENDOR-задачи). `STORAGE_MANAGER_SHIM`
(`crates/js/src/storage_manager.rs:59` — `function StorageManager() {}`) и
`STORAGE_BUCKETS_SHIM` (`crates/js/src/storage_buckets.rs:30,72` —
`function StorageBucketManager() { ... }`, `function StorageBucket(name,
options) { ... }`) объявляют обычные ES5-конструкторы без проверки, что вызов
пришёл из фабрики движка (`navigator.storage = new StorageManager()` /
`self._buckets[name] = new StorageBucket(name, options || {})`), а не со
страницы — обычная JS-функция общедоступна и вызываема с `new` по
определению, нужен явный guard (непубличный символ-капча из фабрики движка
либо превращение конструктора в throw + внутренняя фабричная функция).

## Дальше

Fix scope: заблокировать публичный `new StorageManager()`/`new
StorageBucketManager()`/`new StorageBucket(...)` во всех трёх местах — тот же
паттерн фикса, что предстоит для BUG-629/374/672 (общий guard-механизм имеет
смысл вводить один раз для всех «singleton-only»/«factory-only» интерфейсов
сразу, а не по одному на баг). Не требует TLS-гэпа для воспроизведения/фикса —
живой `--mcp-live-port`-пробы достаточно для верификации.
