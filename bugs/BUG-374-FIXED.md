# BUG-374 — File System Access выставлен как три несвязанных ES5-конструктора вместо WebIDL-иерархии: нет базового `FileSystemHandle`, конструкторы публично вызываемы страницей, `name`/`kind` — перечислимые записываемые собственные свойства вместо readonly-геттеров прототипа, каталог не async-итерируем, `FileSystemWritableFileStream` не наследует `WritableStream`, опции пикеров и пользовательская активация не проверяются вовсе

**Статус:** FIXED 2026-08-10
**Компонент:** js (`crates/js/src/filesystem_access.rs:499-711` — `FSAL_SHIM` целиком)
**Найден:** P2, WPT-VENDOR-file-system-access (2026-07-28), проба `--dump-layout` вне WPT (`.tmp/fsa-probe.html`, `.tmp/fsa-probe2.html`)

## Симптом

Шим определяет три независимые ES5-функции-конструктора без общего предка и без
единого WebIDL-атрибута. Ниже — фактический вывод проб на дефолтном V8; каждый
пункт независим от остальных.

### 1. Нет базового интерфейса `FileSystemHandle`

```
G.FileSystemHandle                = undefined
IDL.instanceof FileSystemHandle   = FileSystemHandle undefined
IDL.proto chain                   = FileSystemFileHandle -> Object
```

По [спеке](https://fs.spec.whatwg.org/#api-filesystemhandle) `FileSystemHandle` —
базовый интерфейс, от которого наследуются `FileSystemFileHandle` и
`FileSystemDirectoryHandle`, и он выставлен на глобале. Сейчас его нет, цепочка
прототипов обрывается на `Object`. Любой feature-detect вида
`if (window.FileSystemHandle)` считает, что File System Access не поддержан.

### 2. Конструкторы публично вызываемы — страница подделывает handle

```
CTOR.FileSystemFileHandle          = constructed name=forged.txt kind=file _token=1
CTOR.FileSystemDirectoryHandle     = constructed name=forged kind=directory _pathId=1
CTOR.FileSystemWritableFileStream  = constructed _id=1
```

Ни у одного из трёх интерфейсов спека не объявляет конструктора — `new
FileSystemFileHandle(...)` обязан бросать `TypeError: Illegal constructor`.
Здесь он не только конструируется, но и принимает внутренние идентификаторы
третьим/вторым аргументом, что делает подделку handle однострочной. Это же —
второй вход в поверхность, описанную в [[BUG-371]].

### 3. `name`/`kind` и внутренние поля — перечислимые записываемые данные

```
IDL.desc kind    = {"value":"file","writable":true,"enumerable":true,"configurable":true}
IDL.desc name    = {"value":"a.txt","writable":true,"enumerable":true,"configurable":true}
IDL.desc _token  = {"value":7,"writable":true,"enumerable":true,"configurable":true}
IDL.kind writable? = directory        // h.kind = 'directory' на файловом handle — принято
IDL.own props of file handle  = ["name","kind","_token","_size"]
IDL.own props of dir handle   = ["name","kind","_pathId"]
```

Спека объявляет `name` и `kind` как `readonly attribute`, то есть геттеры на
прототипе, неперечислимые и незаписываемые. Сейчас это собственные данные
инстанса: `handle.kind = 'directory'` на файловом handle молча принимается, и
объект начинает врать о своём типе. Внутренние `_token`/`_size`/`_pathId` —
там же, наружу (тот же класс, что `__nid__` в [[BUG-367]] и `_map`/`_key` в
[[BUG-369]]).

### 4. Нет `Symbol.toStringTag`

```
IDL.toStringTag                 = undefined
IDL.Object.prototype.toString   = [object Object]
```

Ожидается `[object FileSystemFileHandle]`.

### 5. Отсутствуют члены `FileSystemHandle` и половина членов каталога

```
MEM.file.isSameEntry        = function
MEM.file.queryPermission    = undefined
MEM.file.requestPermission  = undefined
MEM.file.remove             = undefined
MEM.file.move               = undefined
MEM.file.getUniqueId        = undefined

MEM.dir.entries    = function     MEM.dir.getFileHandle      = function
MEM.dir.values     = function     MEM.dir.getDirectoryHandle = function
MEM.dir.keys       = function     MEM.dir.removeEntry        = function
MEM.dir.isSameEntry = function    MEM.dir.resolve            = undefined
```

`resolve()` (обязательный член `FileSystemDirectoryHandle`) отсутствует —
причём у заглушки из `storage_manager.rs` он как раз есть, см. [[BUG-372]].
`queryPermission`/`requestPermission` — основа модели выдачи;
`remove`/`move`/`getUniqueId` покрыты 11 ручными тестами категории.

### 6. Каталог не async-итерируем

```
MEM.dir asyncIterator = undefined
```

`FileSystemDirectoryHandle` объявлен `async iterable<USVString, FileSystemHandle>` —
`for await (const [name, handle] of dir)` обязан работать. Сейчас
`Symbol.asyncIterator` есть только у объекта, возвращаемого `entries()`
(`filesystem_access.rs:592`), но не у самого handle.

Хуже: `entries()` строит дочерние handle-ы с занулёнными внутренними id —
`new FileSystemDirectoryHandle(e.name, 0)` и `new FileSystemFileHandle(e.name, 0, 0)`
(`filesystem_access.rs:587-589`). То есть обход каталога отдаёт объекты с
правильными `name`/`kind` и **неработающим** содержимым: `getFile()` на таком
handle прочитает токен 0 (пусто), `entries()` на подкаталоге вернёт `[]`. Тихая
неверная выдача, а не ошибка.

### 7. `FileSystemWritableFileStream` не поток

```
MEM.writable proto = isWritableStream=false getWriter=undefined locked=undefined abort=undefined
WFS.seek/truncate are no-ops = seek=true truncate=true writeSrcUsesPosition=false
```

По спеке класс наследует `WritableStream` (`getWriter()`, `locked`, `abort()`,
`close()`), а `write()` принимает либо данные, либо словарь
`{type, position, size, data}`. Здесь это самостоятельный объект, `write()`
понимает только строку/`ArrayBuffer` (всё остальное уходит в `String(data)`,
`filesystem_access.rs:512-514` — `Blob` запишется как `"[object Blob]"`), а
`seek()`/`truncate()` (`:519-525`) возвращают resolved-промис и **не делают
ничего**. Позиционная запись и усечение молча теряются — код, усекающий файл
перед записью, получает дописанные данные вместо перезаписанных.

### 8. Нет `FileSystemSyncAccessHandle`

```
G.FileSystemSyncAccessHandle = undefined
```

### 9. Handle не сериализуем

```
SER.structuredClone = cloned, instanceof=false
```

Все три интерфейса помечены `[Serializable]` — handle обязан переживать
`structuredClone`/`postMessage` с сохранением типа и выдачи. Сейчас
`structuredClone` отдаёт простой объект. Это ровно то, что проверяют 11
`local_FileSystemBaseHandle-postMessage-*-manual.https.html`.

### 10. Пикеры не валидируют опции и не требуют пользовательской активации

```
OPT.showOpenFilePicker.length     = 1
OPT.picker src mentions options?  = usesOptions=false len=338
ACT.navigator.userActivation      = object
ACT.picker src mentions activation? = false
```

`showOpenFilePicker(_options)` и `showDirectoryPicker(_options)`
(`filesystem_access.rs:663`, `:689`) принимают словарь и полностью его
игнорируют — подчёркивание в имени параметра это фиксирует.
`showSaveFilePicker` читает только `suggestedName`. Спека требует `TypeError`
на пустом `types`, на MIME без подтипа, с параметрами, с недопустимыми
символами, на неизвестном `startIn`, на `id` длиннее 32 символов и на 20
вариантов некорректного расширения — это 34 из 37 сабтестов
`showPicker-errors.https.window.js`. Оставшиеся 3 требуют `SecurityError` без
transient activation; `navigator.userActivation` в движке есть, но пикеры к
нему не обращаются, поэтому пикер открывается по любому скрипту без жеста
пользователя.

Дополнительно: `FileSystemFileHandle.prototype.createWritable`
(`filesystem_access.rs:556-565`) открывает **save-диалог** вместо записи в тот
же файл, на который выдан handle — то есть запись по существующему handle
всегда требует повторного подтверждения и может уйти в другой файл.

## Ожидается

Полная WebIDL-форма: базовый `FileSystemHandle` с `readonly` геттерами
`kind`/`name` и `isSameEntry`/`queryPermission`/`requestPermission` на
прототипе; наследники без публичных конструкторов (`TypeError: Illegal
constructor`); внутренние id в приватных слотах (`WeakMap`); `Symbol.toStringTag`;
async-итерация самого каталога и корректные id у дочерних handle-ов;
`FileSystemWritableFileStream extends WritableStream` с рабочими
`seek`/`truncate`; валидация опций пикеров и гейт по transient activation.

Объём больше одного среза — разумно разбить: (а) форма + конструкторы + слоты,
(б) валидация опций и активация, (в) `WritableStream`-наследование и
`seek`/`truncate`, (г) `[Serializable]`. Пункт (б) закрывает 34 из 37 сабтестов
`showPicker-errors` и стоит дешевле остальных.

## Измерение в WPT

`tests/wpt/file-system-access/idlharness.https.any.js` — прямой измеритель
пунктов 1-9. В прогоне 2026-07-28 дал TIMEOUT по HTTPS-порт-гэпу (и, отдельно,
`idlharness.js`/`WebIDLParser.js` в дереве не вендорены), поэтому все измерения
выше получены пробой.

## Заметки

- Пробы и вывод целиком: `.tmp/fsa-probe.html`, `.tmp/fsa-probe.log`,
  `.tmp/fsa-probe2.html`, `.tmp/fsa-probe2.log`.
- 34 юнит-теста `filesystem_access::tests` зелены и ни один из этих пунктов не
  видят: они проверяют `typeof X === 'function'` и `… .then === 'function'`,
  то есть наличие имён и форму возвращаемого значения, но не форму объекта и не
  результат (см. [[feedback_green_test_can_mask_broken_feature]]).
- Пункт 2 (публичные конструкторы) — не только конформность: он входит в
  поверхность [[BUG-371]].

## Решение (2026-08-10)

Все десять пунктов закрыты; каждый проверен на реальной сборке пробой
`.tmp/bug374-probe.html` через `--dump-display-list`, не только юнит-тестами.

1-5. **Иерархия WebIDL.** `FileSystemHandle` — база на глобале, от неё
   наследуются оба handle-класса (и прототипы, и сами интерфейсные объекты).
   `kind`/`name` — геттеры на прототипе базы; собственных свойств у инстанса
   ноль (`JSON.stringify(handle) === '{}'`). Конструкторы закрыты приватной
   меткой (`TypeError: Illegal constructor`), внутреннее состояние — в
   `WeakMap`. Есть `Symbol.toStringTag`. Добавлены `queryPermission`,
   `requestPermission`, `remove`, `getUniqueId` (на базе) и `move` (на файле);
   `resolve` был добавлен раньше, в [BUG-372](BUG-372-FIXED.md).

   `isSameEntry` сравнивал токены, а каждый `getFileHandle()` минтит свежий —
   два handle-а на один файл считались разными. Сравнение идёт по стабильной
   метке `_lumen_fs_unique_id`; метка берётся из CSPRNG, а не из хеша пути:
   хеш позволил бы подтвердить угаданный абсолютный путь сравнением дайджестов.

6. **Асинхронная итерация.** `Symbol.asyncIterator` на самом каталоге, а не
   только на объекте из `entries()`. Тихая неверная выдача (дочерние handle-ы с
   занулёнными идентификаторами) закрыта тем, что гранты для элементов минтит
   натив при листинге, наследуя право записи родителя — это же закрывает
   [BUG-750](BUG-750-FIXED.md).

7. **Поток.** `FileSystemWritableFileStream extends WritableStream` рантайма,
   его sink — тот же алгоритм записи. Буфер в Rust перестал быть append-only:
   `write_bytes(id, position, bytes)` и `truncate(id, size)`. Байты идут через
   base64 (строка JS — UTF-16, произвольные байты через неё не проходят), так
   что `Blob`/типизированные массивы больше не превращаются в `String(data)`.
   Понимается словарь `WriteParams`. Команды выстраиваются в очередь синхронно
   в момент вызова: `w.write(...); w.truncate(...); w.close();` без `await` не
   должен давать коммит раньше записи.

8. **`FileSystemSyncAccessHandle`.** Небуферизованный доступ к файлу OPFS через
   реальный открытый дескриптор в Rust: `read`/`write` по позиции,
   `truncate`/`getSize`/`flush`/`close`. Только для файла в песочнице
   происхождения (нужен writable-грант), иначе `NotAllowedError`.

9. **`[Serializable]`.** В `dom.rs` добавлена точка расширения
   `__lumen_platform_cloners`, куда шим регистрирует клонирование handle-ов;
   `structuredClone` платформенного объекта как обычного терял и класс, и грант.

10. **Опции пикеров.** Проверяются MIME без подтипа и с параметрами, расширения
    (регистр, точка на конце, длина), `startIn`, длина и алфавит `id`, пустой
    `types` при `excludeAcceptAllOption`. Пикеры спрашивают
    `navigator.userActivation`.

Попутно, найдено самой пробой: `getFile()` отдавал `File` с размером, который
handle запомнил при создании — после `truncate(5)` страница читала новое
содержимое и старую длину (`hello/0`). Размер перечитывается нативом.

### Что осталось

Гейт по пользовательской активации структурно на месте, но пока не срабатывает:
`navigator.userActivation.isActive` в движке захардкожен в `true`
([BUG-751](BUG-751-OPEN.md)). Это 3 сабтеста из 37 в `showPicker-errors`;
остальные 34 закрыты валидацией опций.

Ещё осталось (вне рамок этой заявки): `createWritable()` на *выбранном* файле
по-прежнему открывает save-диалог — у выбранного файла нет гранта на запись,
и другого пути к нему в текущей модели нет.
