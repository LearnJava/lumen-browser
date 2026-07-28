# BUG-371 — все 10 нативных привязок File System Access / file-input зарегистрированы как обычные глобалы страницы, а их реестры (`FILE_REGISTRY`, `DIR_REG`, `WRITE_REG`) — процесс-глобальные с последовательными целыми id: любая страница перечислением `1,2,3…` читает файлы и каталоги, выданные пользователем другому origin, и дописывает данные в чужой save-handle

**Статус:** OPEN
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
