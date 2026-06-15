# BUG-054

**Статус:** FIXED 2026-06-04
**Компонент:** network
**Файл:** `crates/network/src/lib.rs:662`

## Описание

tests::stale_pooled_connection_triggers_retry падает на Windows (os error 10053 — хост разорвал соединение): тест поднимает loopback TcpListener, кладёт соединение в пул, закрывает сервер и ждёт retry; на Windows закрытое сокет-соединение даёт WSAECONNRESET на read status вместо ожидаемого EOF/retry. Fix: is_stale_error() теперь распознаёт "os error 10053" (WSAECONNABORTED) и "os error 10054" (WSAECONNRESET) — на Windows io::Error форматируется с локализованным OS-сообщением, а не Rust ErrorKind именем.
