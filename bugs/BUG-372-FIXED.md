# BUG-372 — в движке два разных класса `FileSystemDirectoryHandle`, и `navigator.storage.getDirectory()` отдаёт не тот: корень OPFS — замкнутая заглушка из `storage_manager.rs`, она не `instanceof window.FileSystemDirectoryHandle`, у неё нет `entries`/`values`/`keys`/`isSameEntry`, `getFileHandle()` возвращает голый объектный литерал, а `removeEntry()` молча резолвится, ничего не удаляя

**Статус:** FIXED 2026-08-10
**Компонент:** js (`crates/js/src/storage_manager.rs` — заглушечный класс внутри IIFE и `getDirectory`; `crates/js/src/filesystem_access.rs` — настоящий класс и реестр каталогов; `crates/js/src/file_input.rs` — реестр файловых токенов)
**Найден:** P2, WPT-VENDOR-file-system-access (2026-07-28), проба `--dump-layout` вне WPT (`.tmp/fsa-probe.html`)
**Исправлен:** P3, 2026-08-10 — см. «Что сделано» в конце файла

> Номера строк в заголовке и в тексте ниже — на момент заведения заявки.
> Порядок установки к тому времени уже успел смениться на обратный (в
> `v8_runtime.rs::install_dom` `filesystem_access` идёт **раньше**
> `storage_manager`, по алфавиту), из-за чего guard стал истинным «в другую
> сторону»; на суть это не влияло — `getDirectory` всё равно резолвил имя по
> замыканию, и все три следствия воспроизводились ровно как описано.

## Симптом

`storage_manager.rs` определяет собственный `FileSystemDirectoryHandle`
**внутри IIFE**, с комментарием «Full OPFS implementation lives in
filesystem_access.rs; this is a minimal Phase 0 object». Экспортирует он его
под guard-ом:

```js
if (!window.FileSystemDirectoryHandle) {
  window.FileSystemDirectoryHandle = FileSystemDirectoryHandle;
}
```

Guard написан так, будто задача — «не затереть настоящий класс». Но `lib.rs`
ставит `storage_manager` **раньше** (`:1294`), чем `filesystem_access`
(`:1332`), поэтому на момент проверки `window.FileSystemDirectoryHandle` ещё
`undefined` — заглушка всегда экспортируется, а затем всегда затирается
настоящим классом (`filesystem_access.rs:704`, присваивание без guard-а).

Итог: на `window` лежит правильный класс, но `StorageManager.prototype.getDirectory`
(`storage_manager.rs:103`) возвращает `new FileSystemDirectoryHandle('', 'directory')`,
где имя разрешается **по замыканию IIFE** — то есть заглушку. Затирание на
`window` её не затрагивает.

Фактический вывод пробы (дефолтный V8):

```
OPFS.root instanceof window.FileSystemDirectoryHandle = false
OPFS.root ctor name       = FileSystemDirectoryHandle          // одноимённый, но другой
OPFS.root own props       = ["name","kind"]
OPFS.root proto props     = ["constructor","getDirectoryHandle","getFileHandle","removeEntry","resolve"]
OPFS.root members = entries:undefined,values:undefined,keys:undefined,
                    getFileHandle:function,getDirectoryHandle:function,
                    removeEntry:function,resolve:function,
                    isSameEntry:undefined,queryPermission:undefined
```

Два класса с одинаковым `constructor.name` и **непересекающимися** наборами
методов: у настоящего есть `entries`/`values`/`keys`/`isSameEntry`, но нет
`resolve`; у заглушки ровно наоборот. Никакой код не может работать с обоими.

## Три следствия, каждое — тихое

### 1. `getFileHandle()` отдаёт не handle, а объектный литерал

```
OPFS.getFileHandle(create) result = kind=file name=testFile instanceofFSFH=false
                                    ctor=Object getFile=undefined createWritable=undefined
```

`storage_manager.rs:49-51` возвращает `Promise.resolve({ name: name, kind: 'file' })`.
Объект отвечает `kind === 'file'` и `name === 'testFile'` — то есть проходит
любую проверку «похоже на handle» — но не умеет ни `getFile()`, ни
`createWritable()`. Код, который делает `const f = await (await
root.getFileHandle('x')).getFile()`, падает не на границе API, а на шаг позже,
с `getFile is not a function`.

### 2. `removeEntry()` молча резолвится, ничего не удаляя

```
OPFS.removeEntry = resolved (spec: should actually remove)
```

`storage_manager.rs:52-54` — `return Promise.resolve();`. Не отклонение
`NotSupportedError`, не исключение: успешный промис. Вызывающий код считает,
что файл удалён. Это ровно тот класс дефекта, что в
[[feedback_green_test_can_mask_broken_feature]] — тихий неверный ответ
маскируется лучше, чем ошибка. Настоящий класс на этом же вызове честно
отклоняется `NotSupportedError` (`filesystem_access.rs:652-654`).

### 3. `resolve()` всегда `null`

`storage_manager.rs:55-57` — `Promise.resolve(null)`, то есть «этот handle не
является потомком» для любой пары. По спеке `null` означает именно это, так
что отличить «не потомок» от «не реализовано» вызывающий код не может.

### Плюс: обещанные нативы отсутствуют

Док-комментарий `storage_manager.rs:11-14` утверждает, что нативы
`_lumen_storage_estimate()` / `_lumen_storage_persist()` /
`_lumen_storage_get_directory()` «are wired for Phase 1». Проба:

```
NAT._lumen_storage_estimate      = undefined
NAT._lumen_storage_get_directory = undefined
```

Ни один не зарегистрирован — все три `if (typeof … === 'function')` в шиме
(`:71`, `:81`, `:91`, `:100`) всегда ложны, ветки за ними мертвы. Формулировку
дока стоит поправить в том же коммите («заготовлены точки расширения», а не
«wired»).

## Ожидается

По [Storage §9.8 / OPFS](https://fs.spec.whatwg.org/#dom-storagemanager-getdirectory)
`getDirectory()` резолвится **тем самым** `FileSystemDirectoryHandle`, что
выставлен на глобале, со всеми членами `FileSystemHandle` +
`FileSystemDirectoryHandle`.

Правка: выкинуть заглушечный класс из `storage_manager.rs` целиком и вернуть
`new window.FileSystemDirectoryHandle(...)` (либо перенести `getDirectory` в
`filesystem_access.rs`, где класс уже в области видимости, и убрать порядковую
зависимость из `lib.rs`). Отдельный вопрос, который придётся решить тем же
фиксом: корню OPFS нужен реальный `_pathId` из `DIR_REG`, иначе настоящий класс
на нём даст пустой листинг — сейчас заглушка принимает вторым аргументом
`kind`, а настоящий класс — `pathId`, то есть наивная замена конструктора даст
`_pathId === 'directory'`.

## Прямое измерение в WPT

Вендоренный `tests/wpt/file-system-access/getDirectory.https.any.js` — ровно
этот баг, оба его сабтеста:

- `Call getFileHandle successfully` — `directory.getFileHandle(fileName, {create: true})`
  сейчас резолвится объектным литералом, а `t.add_cleanup` вызывает
  `directory.removeEntry(fileName)`, который молча врёт;
- `Call getDirectoryHandle successfully` — то же для подкаталога.

В прогоне 2026-07-28 id `/file-system-access/getDirectory.https.any.html` дал
TIMEOUT по HTTPS-порт-гэпу, до сабтестов дело не дошло — измерение получено
пробой.

## Заметки

- Класс дефекта тот же, что у [[BUG-348]] (`webgl_canvas.rs` перезаписывает
  `getContext` из `dom.rs`): два модуля независимо определяют одно web-имя, и
  наблюдаемый результат зависит от порядка установки в `lib.rs`. Здесь хуже —
  заглушка переживает перезапись, потому что захвачена замыканием.
- Проба и вывод целиком: `.tmp/fsa-probe.html`, `.tmp/fsa-probe.log`.

---

## Что сделано (2026-08-10)

### 1. Второго класса больше нет

Заглушка удалена из `storage_manager.rs` целиком. `FileSystemDirectoryHandle` в
движке ровно один — тот, что определяет `filesystem_access.rs`.

`getDirectory()` берёт его **с `window` в момент вызова**, а не при вычислении
шима:

```js
var Handle = (typeof window !== 'undefined') ? window.FileSystemDirectoryHandle : undefined;
if (!NAT_GET_DIRECTORY || typeof Handle !== 'function') {
  throw new DOMException('The origin private file system is not available', 'NotSupportedError');
}
```

Это снимает не только сам дефект, но и его причину: порядок установки двух шимов
перестал что-либо значить. Если класса нет — вызов **отклоняется**, а не
подставляет похожий объект; ровно этого недоставало, чтобы дефект был заметен.

### 2. У корня OPFS появился настоящий каталог и настоящий грант

Заявка отдельно отмечала, что наивная подмена конструктора не сработает:
заглушка принимала вторым аргументом `kind`, настоящий класс — `pathId`, и
handle без гранта дал бы пустой листинг. Поэтому корень выдаётся как обычная
запись того же `DIR_REG`:

* `filesystem_access.rs::opfs_root_entry_json(origin)` создаёт (при
  необходимости) каталог `<exe_dir>/data/opfs/<origin-slug>/` и аллоцирует на
  него грант — портируемая конвенция CLAUDE.md, не `%APPDATA%`;
* `origin_slug` = читаемый префикс + FNV-1a хеш **полного** origin: непрозрачные
  origin-ы (для `file:`/`data:` это весь URL) бывают длиннее допустимого имени
  каталога, а обрезание префикса ломало бы разделение;
* нативу `_lumen_storage_get_directory` origin передаётся из Rust, страница его
  не называет (правило BUG-371, применённое к корню песочницы). Сам натив шим
  захватывает и **сам же удаляет** с глобала — общий проход
  `seal_file_natives_v8` к этому моменту уже отработал.

### 3. Признак `writable` вместо «create не поддержан»

`DirGrant` получил поле `writable`. Выбранный пользователем каталог остаётся
read-only (диалог спрашивал доступ к тому, что там есть, а не право
переписывать), поддерево OPFS — writable, и подкаталог наследует признак
родителя. На этом держатся все три следствия заявки:

| Было | Стало |
|---|---|
| `getFileHandle(n,{create:true})` → литерал `{name,kind}` | настоящий `FileSystemFileHandle`; в OPFS файл действительно создаётся, в выбранном каталоге — `NotAllowedError` |
| `removeEntry()` → тихо `resolve()` | `_lumen_dir_remove_entry` реально удаляет; непустой каталог без `{recursive:true}` → `InvalidModificationError` |
| `resolve()` → всегда `null` | сегменты пути потомка, `[]` для себя, `null` — только когда это действительно не потомок |

`_lumen_dir_get_file`/`_lumen_dir_get_subdir` получили параметр `create` и вместо
`null` отвечают именем DOMException (`{"error":"…"}`): «нет записи», «грант
read-only» и «это каталог, а не файл» — три разных ответа вызывающему, и
схлопывание их в одно ложное значение было половиной этой заявки.

Сравнение путей для `resolve()` (`_lumen_fs_resolve`) делается в Rust: наружу
по-прежнему уходят только id грантов, ни один путь границу не пересекает.

### 4. Чего фикс не должен был завести

`createWritable()` открывал сохранение через OS-диалог. Для файла из песочницы
это был бы новый тихий неверный ответ — пользователь выбирает место для файла,
которого не видит, а байты уходят не туда, откуда их читает `getFile()` той же
страницы. Поэтому файловый токен тоже получил признак «writable»
(`file_input::register_writable_file_token`), и `_lumen_writable_from_token`
открывает поток прямо на файле песочницы. Токен выбранного пользователем файла
такого потока не даёт — проверено тестом.

Имя записи валидируется в Rust (`valid_entry_name`: пусто, `.`, `..`,
разделители, NUL — отказ `TypeError`). Без этого `{create:true}` означал бы
запись куда угодно, куда дотягивается процесс.

### 5. Дрейф дока

Шапка `storage_manager.rs` больше не утверждает, что
`_lumen_storage_estimate`/`_persist`/`_persisted` «wired»: они так и не
зарегистрированы, это точки расширения. `_lumen_storage_get_directory` теперь
настоящий и описан как настоящий.

### Измерение

Живая проба (`.tmp/opfs-probe.html`, `--dump-display-list`, дефолтная V8-сборка):

```
instanceofGlobalClass=true | ctor=FileSystemDirectoryHandle | name=[] kind=directory
members{entries:function,values:function,keys:function,getFileHandle:function,
        getDirectoryHandle:function,removeEntry:function,resolve:function,isSameEntry:function}
getFileHandle{instanceof=true ctor=FileSystemFileHandle kind=file
              getFile=function createWritable=function}
roundTrip=[lumen opfs bytes]
getDirectoryHandle{instanceof=true name=testDirectory} | resolve=["testDirectory"]
removeEntry=resolved | afterRemove=NotFoundError | removeDir=resolved
```

Оба сабтеста `getDirectory.https.any.js` описывают ровно эту
последовательность (`getFileHandle(create)` + `removeEntry` в cleanup,
`getDirectoryHandle(create)` + `removeEntry(recursive)`); прогнать их через
`wptrunner` по-прежнему мешает HTTPS-порт-гэп, отмеченный в самой заявке, — это
не про этот дефект.

15 юнит-тестов: пути traversal, read-only грант, рекурсивное удаление, `resolve`
во всех четырёх исходах, запись без диалога, отказ на токене выбранного файла,
разделение origin-ов, и три теста на `getDirectory()` (отдаёт глобальный класс /
отклоняется без него / отклоняется без корня).

### Остаток

[BUG-750](BUG-750-OPEN.md) — `entries()`/`values()`/`keys()` отдают handle-ы без
гранта. Для выбранного каталога это сознательное ограничение BUG-371, для OPFS —
бессмысленное; вынесено отдельно, потому что затрагивает модель прав, а не
класс handle-а.
