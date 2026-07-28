# BUG-372 — в движке два разных класса `FileSystemDirectoryHandle`, и `navigator.storage.getDirectory()` отдаёт не тот: корень OPFS — замкнутая заглушка из `storage_manager.rs`, она не `instanceof window.FileSystemDirectoryHandle`, у неё нет `entries`/`values`/`keys`/`isSameEntry`, `getFileHandle()` возвращает голый объектный литерал, а `removeEntry()` молча резолвится, ничего не удаляя

**Статус:** OPEN
**Компонент:** js (`crates/js/src/storage_manager.rs:40-61` — заглушечный класс внутри IIFE, `:99-104` — `getDirectory`, `:59-61` — бесполезный guard `if (!window.FileSystemDirectoryHandle)`; `crates/js/src/filesystem_access.rs:574-659` — настоящий класс; порядок установки `crates/js/src/lib.rs:1294` (storage_manager) → `:1332` (filesystem_access))
**Найден:** P2, WPT-VENDOR-file-system-access (2026-07-28), проба `--dump-layout` вне WPT (`.tmp/fsa-probe.html`)

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
