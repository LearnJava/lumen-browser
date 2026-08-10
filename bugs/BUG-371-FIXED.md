# BUG-371 — все 10 нативных привязок File System Access / file-input зарегистрированы как обычные глобалы страницы, а их реестры (`FILE_REGISTRY`, `DIR_REG`, `WRITE_REG`) — процесс-глобальные с последовательными целыми id: любая страница перечислением `1,2,3…` читает файлы и каталоги, выданные пользователем другому origin, и дописывает данные в чужой save-handle

**Статус:** FIXED 2026-08-10 (P3)
**Компонент:** js (`crates/js/src/filesystem_access.rs:324-405` — макрос `reg!` и восемь `ctx.globals().set(...)`; `:418-486` — те же восемь через `rt.register_native` на V8; `crates/js/src/file_input.rs:104-130` и `:155-171` — `__lumen_file_read_text`/`__lumen_file_read_base64`). Реестры: `file_input.rs:38` (`NEXT_TOKEN: AtomicU64::new(1)`), `file_input.rs:45` (`FILE_REGISTRY`), `filesystem_access.rs:88` (`WRITE_REG`), `filesystem_access.rs:118` (`DIR_REG`)
**Найден:** P2, WPT-VENDOR-file-system-access (2026-07-28), проба `--dump-layout` вне WPT (`.tmp/fsa-probe2.html`)

## Симптом

Модуль `filesystem_access.rs` открывается разделом «# Security model», который
утверждает:

> File paths are never exposed to JS. […] JS only receives an opaque `u64` token.

и `file_input.rs` повторяет:

> Tokens are created **only** by `register_file_token` which is called from Rust […]
> JS can call `__lumen_file_read_text(token)` […] but those only work for
> pre-registered tokens — **they cannot access arbitrary paths**.

Последнее утверждение верно буквально и бесполезно по существу. Пути JS
действительно не видит — но и не нуждается в них: **сами привязки лежат на
`window`**, а пространство токенов — плотный целочисленный ряд с 1.

Фактический вывод пробы (дефолтный V8, обычная локальная страница, никаких
флагов):

```
NAT.__lumen_file_read_text        = function
NAT.__lumen_file_read_base64      = function
NAT._lumen_show_open_file_picker  = function
NAT._lumen_show_save_file_picker  = function
NAT._lumen_show_directory_picker  = function
NAT._lumen_dir_entries            = function
NAT._lumen_dir_get_file           = function
NAT._lumen_dir_get_subdir         = function
NAT._lumen_writable_write_text    = function
NAT._lumen_writable_close         = function

NAT.read_text(1) works w/o grant   = returned ""
NAT.dir_entries(1) works w/o grant = "[]"
```

Обе последние строки — вызовы из страничного скрипта, без единого handle и без
единого диалога. Пустой результат здесь означает не «доступ запрещён», а лишь
«в этом headless-прогоне под id 1 ничего не зарегистрировано»: ни
`read_file_bytes_for_token`, ни `_lumen_dir_entries` не имеют ни проверки
origin, ни проверки выдачи — они просто смотрят в `HashMap` и отдают то, что
там лежит.

## Почему id угадываются

Все три реестра нумеруются с 1 и монотонно:

| Реестр | Счётчик | Что выдаёт по id |
|---|---|---|
| `FILE_REGISTRY` (`file_input.rs:45`) | `NEXT_TOKEN: AtomicU64::new(1)` | путь к файлу → `__lumen_file_read_text(token)` отдаёт его содержимое |
| `DIR_REG` (`filesystem_access.rs:118`) | `next_id: 1` | путь к каталогу → `_lumen_dir_entries(id)` листает его, `_lumen_dir_get_file(id, name)` **выпускает новый файловый токен** |
| `WRITE_REG` (`filesystem_access.rs:88`) | `next_id: 1` | путь для записи → `_lumen_writable_write_text(id, data)` + `_lumen_writable_close(id)` пишут файл |

Ни один из счётчиков не сбрасывается при навигации: `OnceLock`/`LazyLock`
делают их процесс-глобальными, а `file_input.rs:40-44` явно фиксирует это как
намеренное («A process-global `Mutex` shares it correctly across both threads»).
После того как пользователь один раз выбрал файл через `<input type=file>` или
пикер — на любой странице, в любой вкладке, любого origin — токен остаётся
живым до конца процесса.

## Три конкретных сценария

1. **Чтение чужого выбора.** Пользователь на `bank.example` приложил выписку
   через `<input type=file>` (шелл вызвал `register_file_token`, токен = N).
   Затем открывает `evil.example`. Скрипт делает
   `for (var i=1;i<1000;i++) send(__lumen_file_read_text(i))` — и вычитывает
   содержимое всех файлов, выданных за сессию. Никакого handle, никакого
   пользовательского жеста.
2. **Обход каталога и эскалация до чтения.** Если пользователь когда-либо
   подтвердил `showDirectoryPicker()`, `_lumen_dir_entries(1)` возвращает
   листинг каталога, а `_lumen_dir_get_file(1, "<имя из листинга>")`
   **регистрирует новый файловый токен** (`filesystem_access.rs:373-380`,
   `file_entry_json` → `register_file_token`) — то есть страница сама себе
   выпускает право чтения на любой файл внутри выданного каталога, включая тот,
   который пользователь не показывал. `_lumen_dir_get_subdir` рекурсивно
   раскрывает поддеревья.
3. **Перехват записи.** `showSaveFilePicker()` на честной странице выделяет
   `WRITE_REG` id (обычно 1). До вызова `.close()` любая другая страница может
   вызвать `_lumen_writable_write_text(1, "<произвольные байты>")` и
   `_lumen_writable_close(1)` — содержимое, которое пользователь согласился
   сохранить, подменяется, а файл записывается по подтверждённому им пути
   (`WriteRegistry::close` → `std::fs::write`, `filesystem_access.rs:79-85`).

## Смежное: `File._token` — обычное перечислимое записываемое свойство

Даже без прямого вызова нативов тот же доступ достижим через `File`:

```
TOK.File _token settable      = desc={"value":12345,"writable":true,"enumerable":true,"configurable":true} text=function
TOK.forged File.text() resolves = promise=true
TOK.File ctor accepts _token option = _token=77
```

`File.prototype.text` (`file_input.rs:211-222`) читает ровно `this._token`, а
конструктор `File` (`file_input.rs:193`) принимает `_token` прямо в
options-словаре. То есть `new File([], 'x', {_token: 42}).text()` — это
`__lumen_file_read_text(42)` в один шаг. То же верно для публичных
конструкторов handle-ов (см. [[BUG-374]]): `new FileSystemFileHandle('x', 42, 0)`
конструируется страницей, потому что WebIDL-скрытие конструкторов не сделано.

## Ожидается

По [File System Access §5](https://wicg.github.io/file-system-access/) и
[File API](https://w3c.github.io/FileAPI/) выдача привязана к handle-объекту,
handle не подделывается страницей, а выдача — к origin.

Минимум, закрывающий все три сценария:

1. Нативы не должны быть свойствами `window`. Остальной код шима обращается к
   ним по имени, поэтому нужен либо приватный слот (как в других модулях —
   вызов через замыкание, а не глобал), либо переименование с последующим
   `delete` после установки шима. Голое переименование в `__`-имя ничего не
   даёт: проба находит их именно перечислением известных имён, но и
   `Object.getOwnPropertyNames(window)` их покажет.
2. id должны быть неугадываемыми (например `u128` из CSPRNG) — это дешёвая
   половина фикса, снимающая перебор, но не разделение origin.
3. Реестры должны хранить origin выдачи и сверять его при каждом обращении;
   при навигации записи origin-а должны инвалидироваться. Без этого пункт 2
   лишь усложняет атаку, а не исключает её.
4. `_token` не должен быть web-видимым: перенести в приватный слот (`WeakMap`
   по объекту `File`), и убрать приём `_token` из публичного конструктора
   `File`.

## Заметки

- Пункты 1 и 4 — механические и не меняют поведение легитимного кода.
  Пункты 2-3 требуют решения о том, где хранится origin в момент вызова натива
  (JS-поток уже отделён от UI-потока, см. комментарий `file_input.rs:40-44`).
- Проба и вывод целиком: `.tmp/fsa-probe2.html`, `.tmp/fsa-probe2.log`.
- Ни один WPT-тест категории `file-system-access` этого не ловит и поймать не
  может — это не расхождение со спекой, а лишняя поверхность сверх неё. Все 5
  id категории в прогоне 2026-07-28 упёрлись в HTTPS-порт-гэп (TIMEOUT).

---

## Фикс (P3, 2026-08-10)

Закрыты все четыре пункта раздела «Ожидается».

### Где хранится origin (решение по вопросу из «Заметок»)

Не в момент вызова натива — **в момент установки биндингов**. `install_dom`
уже получает `page_url` документа, поэтому origin выводится там
(`file_input::origin_for_url`) и **захватывается в замыкание каждого натива на
Rust-стороне**. Страница не может ни прочитать его, ни подсунуть другой: в
отличие от натинов Service Worker / Cache API, где origin — обычный аргумент из
JS, здесь его в сигнатуре нет вовсе. Разделение потоков роли не играет —
значение фиксируется один раз при установке, а реестры как были
процесс-глобальными под `Mutex`, так и остались.

`origin_for_url` даёт tuple-origin `scheme://host[:port]` для URL с хостом и
полную строку URL для всего остального (`file:`, `data:`, `about:`) — у них по
спеке opaque origin, и полный URL строже общей корзины `"file://"`: две
локальные страницы не наследуют гранты друг друга.

### 1. Нативы не свойства `window`

Оба шима копируют нужные им привязки в переменные замыкания в самом начале,
после чего `install_dom` зовёт `file_input::seal_file_natives_v8` и удаляет с
глобала все 11 имён — десять нативов плюс мост `__lumen_fs_internal`. Удаление
живёт в `install_dom`, а не в хвосте одного из шимов: установки best-effort, и
при падении любой из них поверхность всё равно обязана исчезнуть.

`FSAL_SHIM` ради этого переехал в IIFE (был на верхнем уровне); классы
по-прежнему доступны как глобалы, потому что `window.X = X` на реальной странице
и есть глобал.

### 2. Неугадываемые id

`new_grant_id()` — 128 бит `getrandom` в виде 32 hex-символов. Токен стал
JS-**строкой**: `f64` такую величину не несёт, а расширять прежний `u64`-счётчик
было нельзя без тихой потери точности. Соответственно все десять нативов приняли
строковый параметр, `file_entry_json` отдаёт `"token":"…"`, а
`entries_to_json_with_tokens` в шелле квотирует значение. Если OS-энтропия
недоступна, грант не выдаётся вовсе (пустая строка, которая ничего не открывает)
— предсказуемого фолбэка нет по определению.

### 3. Origin в реестрах + инвалидация при навигации

Каждая запись `FILE_REGISTRY`/`DIR_REG`/`WRITE_REG` хранит origin выдачи и
сверяется при каждом обращении. Установка биндингов документа сначала отзывает
гранты предыдущего документа того же origin
(`revoke_grants_for_origin` / `DirRegistry::revoke_origin` /
`WriteRegistry::revoke_origin`), поэтому токен не переживает страницу, которой
был выдан.

Шелл (`open_file_picker`) регистрирует путь под
`file_input::active_document_origin()` — читает то, чем реально
проинициализировался рантайм, вместо повторного вывода origin из `PageSource`.
Второй вывод молча разъехался бы с первым (`page_url` для `PageSource::File`
строится как `file://{path}`, а `PageSource::url_str()` для него отдаёт `None`),
и это не упало бы, а просто вернуло пустую строку из каждого чтения.

### 4. `_token` невидим для web

Токен `File` уехал в `WeakMap` внутри IIFE `FILE_INPUT_SHIM`, а конструктор
`File` больше не читает `options._token` — прицепить грант можно только через
внутреннюю фабрику `makeTokenFile`, недостижимую после sealing. Тем же приёмом
закрыты внутренности handle-ов (`_token`/`_size`, `_pathId`, `_id`/`_closed`) —
это ровно та же поверхность, отмеченная в заявке как «то же верно для публичных
конструкторов handle-ов».

## Верификация

A/B той же пробой (`--dump-display-list`) на бинарнике до фикса и после:

| Замер | до | после |
|---|---|---|
| нативы достижимы из скрипта | 10-of-11 | 0-of-11 |
| нативы в `Object.getOwnPropertyNames(window)` | 10-of-11 | 0-of-11 |
| `new File([],'x',{_token:12345})._token` | `12345` | `undefined` |
| дескриптор `_token` на `File` | present | absent |
| собственные свойства handle-ов | `[name\|kind\|_token\|_size][name\|kind\|_pathId][_id\|_closed]` | `[name\|kind][name\|kind][]` |
| `showOpenFilePicker`/`showSaveFilePicker`/`showDirectoryPicker`/`File`/`FileList`/`FileSystemFileHandle` | все `function` | все `function` |

13 новых тестов поверх обновлённых прежних: неугадываемость и уникальность
токенов, отказ чужому origin на чтении/записи/обходе каталога, отзыв при
установке, невидимость приватных слотов, работоспособность `File.text()` **после**
sealing. `cargo test -p lumen-js --features v8-backend --lib -- file_input::
filesystem_access::` — 63/63.

## Остаток

Форма WebIDL (скрытые конструкторы, `FileSystemHandle`-база, readonly-геттеры)
остаётся за [BUG-374](BUG-374-FIXED.md) — здесь она не трогалась: после того как
id стали неугадываемыми и origin-связанными, публичный конструктор handle-а
больше не даёт доступа, только форму.
