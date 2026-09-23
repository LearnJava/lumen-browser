# BUG-789 — `stale_pooled_connection_triggers_retry` флакует на macOS

**Статус:** FIXED 2026-09-23 (P6)
**Компонент:** network (`crates/network/src/lib.rs::is_stale_error`; тест
`tests::stale_pooled_connection_triggers_retry`, `crates/network/src/lib.rs:6674`)
**Найден:** 2026-08-18 (P5), CI-прогон `32155694312` на ветке `p5-ci-cache`.
Выделен в отдельный баг 2026-08-20 (P2) при закрытии [BUG-783](BUG-783-FIXED.md),
где был описан как «второй случай того же класса» — но механизм другой, отдельным
фиксом не покрывается.

## Симптом

CI-прогон `32155694312`: ubuntu и windows зелёные, macOS красный на
`lumen_network::tests::stale_pooled_connection_triggers_retry` —
`unwrap()` на `Result<Vec<u8>, Error>` в `crates/network/src/lib.rs:6726`
(второй `client.fetch(&url)`, тот, что должен пройти через retry-on-stale).
Перезапуск **только упавшего джоба** (`gh run rerun --failed`) — зелёно;
правка в том коммите касалась исключительно checkout и кэша и на логику теста
влиять не может.

## Механизм

Тест поднимает локальный сервер в отдельном потоке: первое соединение отдаёт
ответ без `Connection: close` (сервер «врёт» про keep-alive) и сразу закрывает
сокет — имитация серверного idle-timeout. Клиент должен заметить
stale-write/read при повторном использовании пула и открыть новое соединение
(`retry-on-stale`). Ожидается 2 `accept()`, оба fetch успешны.

**Найденная причина (P6, 2026-09-23): не гонка в примитиве синхронизации, а
сама классификация ошибки.**

`is_stale_error` решала, является ли ошибка «stale keep-alive», строковым
поиском по `format!("{err:?}")`: искала camelCase-литералы `ErrorKind` —
`"BrokenPipe"`, `"ConnectionReset"`, `"ConnectionAborted"`, `"UnexpectedEof"`.
Но все ~80 сайтов конвертации `io::Error` → `Error::Network` в этом крейте
(`crates/network/src/http1/request.rs`, `http1/response.rs`, `http1/chunked.rs`
и т.д.) форматируют исходную ошибку через `Display`
(`format!("read status: {e}")`), не `Debug`. `Display` для `io::Error` даёт
текст `strerror`, а не имя `ErrorKind`: на Unix — `"Connection reset by peer
(os error 104)"`, `"Broken pipe (os error 32)"`, `"Software caused connection
abort (os error 103)"`. Литералы вида `"ConnectionReset"` в таком тексте
**никогда не встречаются** — проверка была мертва для этого класса ошибок на
любой платформе, кроме двух явных лазеек:

1. Чистый EOF (`n == 0` из `read_line`/`read_exact`) — код сам печатает
   литеральные строки `"EOF before status line"` / `"EOF in headers"`
   (`crates/network/src/http1/response.rs:56,70`), их находит `msg.contains`.
2. Windows — `io::Error::from_raw_os_error` там форматируется с
   локализованным OS-сообщением, поэтому в `is_stale_error` заранее стоял
   числовой fallback: `"os error 10053"` (WSAECONNABORTED) / `"os error
   10054"` (WSAECONNRESET), который матчится независимо от языка ОС.

На macOS/Linux, когда сервер закрывает соединение не чистым FIN (клиент
видит EOF → путь 1 срабатывает), а RST/EPIPE (клиент видит `ECONNRESET`/
`EPIPE`/`ECONNABORTED`), сообщение не попадало ни под один литерал — ошибка
пробрасывалась наверх как настоящий отказ вместо retry. Какой именно исход
(FIN или RST) получает клиент, зависит от таймингов ОС в момент
`shutdown()`+`close()` на сервере относительно состояния сокета на клиенте —
отсюда наблюдаемая «гонка»: тест иногда попадал в путь 1 (зелёный), иногда
в непокрытый путь (красный).

**Отличие от [BUG-783](BUG-783-FIXED.md):** там тест сам ждал события
(регистрации перехвата) фиксированным опросом — чинилось заменой опроса на
`Condvar`. Здесь у теста такого опроса нет, гонка была не в тесте и не в
таймингах синхронизации вообще, а в классификации уже полученной ошибки.

## Исправление

`is_stale_error` переписана на регистронезависимое сравнение с реальными
подстроками `strerror`, которые действительно появляются в `Display`-тексте:
`"broken pipe"`, `"connection reset"`, `"connection abort"` (плюс
`"unexpected end of file"`, `"eof before status line"`, `"eof in headers"`,
lowercased). Windows-numeric fallback (`"os error 10053"`/`"10054"`) оставлен
без изменений — там причина другая (локализация), не формат `Display` vs
`Debug`.

Тест `is_stale_error_recognises_eof_and_resets` переписан под настоящие
`Display`-строки (`"Connection reset by peer (os error 104)"` и т.п. вместо
вымышленных `"ConnectionReset"`).

## Проверка

- `cargo test -p lumen-network --lib is_stale_error_recognises_eof_and_resets` — зелёный.
- `cargo test -p lumen-network --lib stale_pooled_connection_triggers_retry` — зелёный, 3× подряд (`--test-threads=1`).
- `cargo clippy -p lumen-network --all-targets -- -D warnings` — чист.

macOS-раннер недоступен в этой среде (Windows) — фикс устраняет найденный
механизм ложноотрицательной классификации ошибки, подтверждённый чтением
кода (все сайты конвертации грепом проверены на `{e}` vs `{e:?}`), но живой
macOS CI-прогон не воспроизведён напрямую. Если флак повторится, следующий
шаг — проверить, не появился ли на конкретном раннере текст `strerror`,
отличный от учтённых трёх подстрок.
