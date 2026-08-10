# BUG-389 — File System API: `FileSystemSyncAccessHandle`, `FileSystemObserver`, `FileSystemHandle.move()` не реализованы вовсе

**Статус:** OPEN
**Компонент:** js (`crates/js/src/filesystem_access.rs`, `crates/js/src/storage_manager.rs`
— обе точки установки `FileSystem*`-классов; ни в одной нет упомянутых членов)
**Найден:** P2, WPT-VENDOR-fs (2026-07-28), проба `--mcp-live-port` (`.tmp/probe_fs.py`)

## Симптом

```
typeof window.FileSystemSyncAccessHandle                                => "undefined"
typeof window.FileSystemObserver                                        => "undefined"
typeof window.FileSystemHandle                                          => "undefined"
typeof window.FileSystemFileHandle.prototype.createSyncAccessHandle     => "undefined"
typeof window.FileSystemFileHandle.prototype.move                       => "undefined"
```

Ни один из членов, вокруг которых построена вся WPT-категория `fs` (88
вендоренных файлов), не реализован: класс `FileSystemSyncAccessHandle`
(синхронный доступ к файлу из Worker — `read`/`write`/`truncate`/`getSize`/
`flush`/`close`), `FileSystemFileHandle.prototype.createSyncAccessHandle()`
(точка получения такого хендла), `FileSystemObserver` (наблюдение за
изменениями в OPFS — `observe()`/`disconnect()`, callback с записями
`FileSystemChangeRecord`), и `FileSystemHandle.prototype.move()` (переименование/
перемещение хендла на месте, без пересоздания). Также отсутствует общий базовый
класс `FileSystemHandle` — `FileSystemFileHandle`/`FileSystemDirectoryHandle`
не имеют общего прототипа, что и служит спекового «instanceof FileSystemHandle».

Сам прогон `run_report.py --all --root fs --recursive` (32 отобранных id) не
дал сигнала — 100% TIMEOUT по HTTPS-порт-гэпу (все 42 тестовых файла
категории — `.https.`, `testdriver` не встречается ни разу, вариантов нет), см.
[docs/wpt-status.md](../docs/wpt-status.md). Находка — целиком с
`--mcp-live-port`-пробы, не с прогона.

## Причина

Не баг реализации, а нереализованная область: `crates/js/src/filesystem_access.rs`
покрывает только picker-based File System Access API (`showOpenFilePicker` и
производные — showcased в BUG-371..374), исходно это Phase 0 срез. Синхронный
доступ (нужен воркерам для быстрого локального стораджа — SQLite-в-браузере,
OPFS-based БД) и `FileSystemObserver` (Storage §9) в кодовой базе не
затрагивались вовсе — ни строки, ни заглушки, ни комментария "Phase 1".

## Влияние

* Категория `fs` целиком про этот функционал — **0% API-поверхности
  реализовано**, что делает саму HTTPS-порт-гэп стену неактуальной: даже с
  портами страница не продвинулась бы дальше первого обращения к
  `createSyncAccessHandle`/`FileSystemObserver`.
* Синхронный доступ к OPFS — предпосылка для клиентских БД в браузере
  (`sql.js`/wasm-SQLite поверх OPFS, IndexedDB-alternative паттерны), популярный
  сценарий у privacy-ориентированных SPA; без него такие страницы падают в
  фоллбэк на IndexedDB или полностью не работают.
* `FileSystemHandle.move()` — единственный способ атомарного переименования по
  спеке; без него код вынужден эмулировать через copy+delete, что не
  атомарно и не покрыто WPT-путём, которым Lumen сейчас идёт (`filesystem_access.rs`
  вообще не даёт такой операции).

## Как чинить

Отдельный Phase-1/2 срез, не входит в объём этой WPT-вендор-задачи (P2 —
только вендорит и документирует, не реализует API). При реализации:

1. Ввести общий `FileSystemHandle` как базовый прототип для `FileSystemFileHandle`/
   `FileSystemDirectoryHandle` (см. также [BUG-372](BUG-372-FIXED.md) — сначала
   унифицировать существующий дубль классов, потом наращивать иерархию).
2. `createSyncAccessHandle()` — синхронная операция, доступна только в
   `DedicatedWorkerGlobalScope`/`SharedWorkerGlobalScope` по спеке; в
   однопоточной модели Lumen (текущий JS-движок не даёт настоящих Worker-потоков
   с блокирующим FS-доступом) потребует отдельного проектного решения, не
   тривиальной привязки.
3. `FileSystemObserver` — типовой `observe()`/`disconnect()` плюс callback-очередь
   уровня микротаска; закладка на будущий watcher поверх файловой системы ОС.

## Связанные

* [BUG-372](BUG-372-FIXED.md) — та же пара модулей, `getDirectory()` отдаёт
  несовместимый теневой класс (найден раньше, на категории `file-system-access`;
  здесь тот же дефект подтверждён повторно, независимой пробой, на категории `fs`).
* [BUG-371](BUG-371-FIXED.md)…[BUG-374](BUG-374-FIXED.md) — соседняя категория
  `file-system-access` (picker-based API), тот же модуль `filesystem_access.rs`.
