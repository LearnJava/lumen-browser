# BUG-679: HTTP 1xx informational responses (103 Early Hints) treated as terminal instead of skipped

**Статус:** FIXED 2026-08-06
**Дата:** 2026-08-06
**Компонент:** network (`crates/network/src/lib.rs::read_head`, `crates/network/src/h2/conn.rs::H2Conn::fetch`/`read_response_for_stream`)
**Найден:** P3 живой проверкой при расследовании [BUG-331](BUG-331-FIXED.md) (Cloudflare 503-заявка) — `https://www.cloudflare.com/` использовалась как дополнительный Cloudflare-защищённый сайт для сверки TLS-гипотезы.

## Симптом

```
target/dev-release/lumen.exe --dump-source https://www.cloudflare.com/
→ GET https://www.cloudflare.com/
← 103 https://www.cloudflare.com/
Ошибка dump https://www.cloudflare.com/: network error: HTTP 103
```

cloudflare.com отдаёт `103 Early Hints` (RFC 8297) — промежуточный ответ, который сервер шлёт ДО финального `200`, чтобы браузер начал грузить преднагружаемые ресурсы раньше. Клиент обязан прозрачно пропустить его и дождаться финального ответа (RFC 9110 §15.2). Lumen вместо этого возвращал `103` как если бы это был окончательный статус и валил fetch как сетевую ошибку.

## Root cause

Два независимых места, оба страдают одной и той же логической ошибкой — «декодировать статус один раз, не различая промежуточный/финальный»:

1. **HTTP/1.1** (`crates/network/src/lib.rs::read_head`): читал ровно один блок status-line+headers и возвращал его как единственный результат. Любой 1xx-статус (кроме случая явного апгрейда, который идёт по отдельному пути `websocket::upgrade::expect_101`) улетал наверх в `fetch_with_redirect`'s catch-all `status => return Err(...)` (lib.rs:2652).

2. **HTTP/2** (`crates/network/src/h2/conn.rs`): `fetch()` и `read_response_for_stream()` копили байты ВСЕХ HEADERS/CONTINUATION-фреймов данного стрима в один буфер и декодировали HPACK **один раз**, после `end_stream`. Промежуточный `103`-блок (END_HEADERS=true, END_STREAM=false) и финальный `200`-блок (следующая HEADERS-последовательность на том же stream_id) склеивались в один HPACK-байтовый поток. HPACK-декодер не падает на такой склейке — он просто линейно декодирует инструкции одну за другой, отдавая ОБА набора полей одним плоским списком; `:status` резолвился через `.find()` (первое совпадение) — то есть побеждал более ранний `103`, а не настоящий `200`. Реальные заголовки (`content-type` и т.д.) финального ответа при этом тоже терялись бы (перекрывались бы фильтром `!name.starts_with(":")`  без разбора, откуда они) — баг был не только про статус.

## Фикс

- `read_head` (lib.rs): цикл до `MAX_INTERIM_RESPONSES=20` — читает status-line+headers; если статус `100..200` (и не `101`, который остаётся терминальным для апгрейд-запросов), блок отбрасывается и читается следующий. `101` не проходит через эту функцию (WebSocket-апгрейд разбирается отдельным `expect_101`), поэтому явно исключён из диапазона отбрасывания.
- `H2Conn::fetch`/`read_response_for_stream` (h2/conn.rs): статус декодируется СРАЗУ, как только `end_headers` завершает текущий блок, а не в конце всего стрима. Если статус `1xx` — декодированные поля отбрасываются, аккумулятор (`hdr_block`/`StreamState.hdr_block`) сбрасывается для следующего блока, цикл продолжается. Финальный (не-1xx) статус фиксируется в `final_status`/`StreamState.final_status`, и именно он возвращается вызывающему коду. HPACK-декодер остаётся в правильном стейте (dynamic table), потому что `decode()` всё равно вызывается на каждом complete-блоке — просто результат для 1xx-блоков отбрасывается, а не сам вызов.

## Регресс-тесты

- `crates/network/src/lib.rs::tests::fetch_skips_1xx_interim_response_before_final` — два `103`-ответа подряд перед финальным `200` на HTTP/1.1-mock-сервере.
- `crates/network/src/h2/conn.rs::tests::fetch_skips_1xx_informational_headers_before_final_status` — HTTP/2 `fetch()`, один `103`-блок перед `200`.
- `crates/network/src/h2/conn.rs::tests::concurrent_read_response_for_stream_skips_1xx_informational_headers` — тот же кейс через concurrent-API (`send_request`/`read_response_for_stream`).

Полный `cargo test -p lumen-network`: 2137/2137 (было 2134 до добавления трёх новых тестов), `cargo clippy -p lumen-network --all-targets -- -D warnings` чист.

## Живая проверка

```
target/dev-release/lumen.exe --dump-source https://www.cloudflare.com/
→ GET https://www.cloudflare.com/
← 200 https://www.cloudflare.com/
Получено 1305416 байт
```

До фикса: `network error: HTTP 103`. После: `200`, 1.3 МБ реального контента.
