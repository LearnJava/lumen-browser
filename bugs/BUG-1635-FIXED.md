# BUG-1635 — headless `--mcp-port` не учитывает `--proxy`

**Статус:** FIXED 2026-10-18 (P6)
**Заведён:** 2026-10-18 (Chat, при разборе [BUG-1239](BUG-1239-OPEN.md))
**Область:** shell / driver (`crates/shell/src/automation_server.rs`, `InProcessSession` в `crates/driver`)

## Симптом

`lumen.exe --maximized --proxy http://127.0.0.1:12334 --mcp-port <N> https://www.youtube.com/`:

```
повтор после сорванного TLS-рукопожатия: https://www.youtube.com/ (network error: TLS handshake: … (os error 10060))
MCP: ошибка загрузки https://www.youtube.com/: network error: TLS handshake: …
```

В этой сети прямое соединение с youtube рвётся на TLS; то же окно без `--mcp-port`, но с тем же `--proxy`,
грузит страницу (`200`). Значит, headless-сессия открывает соединения мимо прокси из `startup_profile`
(ср. BUG-1210 — `--proxy`/`--tor` складываются в профиль до запуска режимов).

Попутно: строка токена в этом режиме — `MCP token: …`, в живом — `[mcp] token: …`; `scripts/scroll_perf.py`
ищет только вторую.

## Что сделать

1. Передать прокси (HTTP и SOCKS5/`--tor`) в `HttpClient` headless-сессии `--mcp`/`--mcp-port`
   (и `--ipc-server`, если там тот же путь).
2. Унифицировать строку токена или научить клиентов обеим.
3. Тест: headless-сессия с прокси-заглушкой на loopback — запрос страницы проходит через неё.

**Готово, когда:** `--mcp-port` с `--proxy` грузит youtube в этой сети; тест зелёный.

## Исправление

- `InProcessSession::set_http_client_hook` (`crates/driver/src/session.rs`) — доводка клиента, собираемого на каждый `http(s)`-`navigate`.
- `FingerprintProfile::apply_proxies` (`crates/shell/src/config.rs`) — прокси/SOCKS5/AIA вынесены из `apply_http`.
- `run_mcp_mode` вешает хук с глобальным профилем; `--ipc-server` не создаёт `InProcessSession`, правки не требует.
- Строка токена везде `[mcp] token: …` (`automation_server.rs`, `crates/mcp/src/main.rs`, `docs/automation.md`).
- Тест: `automation_server::tests::headless_session_fetches_through_profile_proxy` — заглушка на loopback получает `GET http://…:80/…`.
