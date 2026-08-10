# BUG-389 — File System API: `FileSystemSyncAccessHandle`, `FileSystemObserver`, `FileSystemHandle.move()` не реализованы вовсе

**Статус:** FIXED 2026-08-10
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

## Как починено

Закрыт в три захода, потому что заявка описывает не один дефект, а целую
непокрытую поверхность:

1. **Иерархия и `move()`** — [BUG-374](BUG-374-FIXED.md) (2026-08-10) ввёл общий
   базовый `FileSystemHandle` (`inherit()` ставит и цепочку прототипов, и
   `Object.getPrototypeOf(FileSystemFileHandle) === FileSystemHandle`) и
   `FileSystemFileHandle.prototype.move()` поверх нативного `_lumen_fs_move`.
2. **`FileSystemSyncAccessHandle` + `createSyncAccessHandle()`** — там же, поверх
   реального дескриптора OPFS (`_lumen_sync_open`/`read`/`write`/`truncate`/
   `getSize`/`flush`/`close`), позиционные `read`/`write` через base64.
3. **`FileSystemObserver`** (этот коммит) — последнее, чего не было вовсе.

Про третий пункт: ОС-вотчера под ним нет и зависимость ради него не заводилась.
`observe()` снимает слепок наблюдаемого поддерева (`snapshot_tree`), опрос
сравнивает следующий слепок с предыдущим (`diff_snapshots`). Всё, что описывает
FS Observer §2, из двух слепков выводится — **кроме тождества переименованного
файла**: `rename` неотличим от пары `disappeared`+`appeared`. Поэтому `moved`
докладывается только когда опрос увидел ровно одно исчезновение и одно появление
с совпадающими метаданными (вид/длина/mtime — их сохраняет `rename` и не
сохраняет свежая запись); неоднозначные случаи остаются двумя честными записями,
а не одной выдуманной.

Слепок намеренно не полный `Metadata`: сравнение времени доступа докладывало бы
`modified` на обычное чтение.

Состояние наблюдений живёт в Rust (`OBS_REG`), а не в JS: страница, отпустившая
наблюдателя, должна перестать стоить обход каталога, а проверка origin обязана
происходить там же, где реестры грантов. Один общий `setInterval` на все
наблюдения — таймер на наблюдение умножал бы обходы одного дерева на число
наблюдателей. Установка для нового документа снимает наблюдения предыдущего тем
же правилом, что и гранты ([BUG-371](BUG-371-FIXED.md) п.3).

Пункт 2 «Как чинить» (синхронный доступ только из воркера) сознательно не
исполнен: `createSyncAccessHandle()` не гейтится по глобальной области, потому
что в однопоточной модели Lumen отдельного `DedicatedWorkerGlobalScope` с
блокирующим FS-доступом нет — гейт отклонял бы единственный контекст, из
которого API вообще достижимо.

Тесты — 19 в `filesystem_access::tests_v8`, включая `bug_389_symptom_list_is_gone`
(все пять `typeof`-строк симптома разом). JS-уровень опрашивается через
управляемый `__tick()` вместо часов: харнесс подменяет `setInterval` сбором
колбэков, так что «прошёл интервал опроса» — это шаг теста, а не ожидание.

## Осталось

* Ставка опроса — 100 мс, наблюдение стоит обход поддерева на тик. Для больших
  деревьев это заметно; настоящий ОС-вотчер (`ReadDirectoryChangesW`/`inotify`)
  снял бы и цену, и задержку — отдельная задача, не этот баг.
* Категория WPT `fs` по-прежнему не даёт сигнала: 100% TIMEOUT по HTTPS-порт-гэпу
  (все 42 файла — `.https.`). Перемер осмысленен только после закрытия гэпа, и он
  за P2 — эта правка снимает вторую стену (отсутствие API), но не первую.

## Связанные

* [BUG-372](BUG-372-FIXED.md) — та же пара модулей, `getDirectory()` отдаёт
  несовместимый теневой класс (найден раньше, на категории `file-system-access`;
  здесь тот же дефект подтверждён повторно, независимой пробой, на категории `fs`).
* [BUG-371](BUG-371-FIXED.md)…[BUG-374](BUG-374-FIXED.md) — соседняя категория
  `file-system-access` (picker-based API), тот же модуль `filesystem_access.rs`.
