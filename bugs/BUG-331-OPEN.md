# BUG-331: Cloudflare-защищённые сайты возвращают `HTTP 503` для Lumen, хотя HTTP-заголовки корректно подделаны под браузер

**Статус:** OPEN
**Дата:** 2026-07-23
**Компонент:** network (`crates/network/src/tls/fingerprint.rs`, `crates/network/src/http/headers.rs`, `crates/network/src/http/mod.rs`)
**Найден:** эксперимент с автозаполнением веб-формы через MCP (`--mcp-live-port`, инструменты `type`/`click`/`query`) на публичной тестовой форме httpbin.org/forms/post.

## Симптом

`target/dev-release/lumen.exe --dump-source https://httpbin.org/forms/post` стабильно возвращает `503`:

```
→ GET https://httpbin.org/forms/post
← 503 https://httpbin.org/forms/post
Ошибка dump https://httpbin.org/forms/post: network error: HTTP 503
```

Воспроизведено **4 из 4** попыток (headless `--dump-source` × 3, живое окно через `--mcp-live-port` × 1).

При этом:
- `curl -A "Lumen/0.5.0" https://httpbin.org/forms/post` (та же User-Agent-строка, что шлёт Lumen — см. `DEFAULT_USER_AGENT` в `crates/network/src/http/mod.rs:30`) получает `200` каждый раз.
- `curl` без переопределения UA — `000` (сетевая ошибка на стороне самого curl-вызова в тестовом окружении, не относится к делу).
- `target/dev-release/lumen.exe --dump-source https://github.com/` (тоже за CDN с анти-бот-защитой) — `200`, 579345 байт настоящего контента. Значит, сеть/TLS-стек Lumen в целом рабочий, это не тотальный сбой.
- `target/dev-release/lumen.exe --dump-source https://example.com` — `200`, штатно.

## Анализ

`crates/network/src/http/headers.rs` реализует полноценный набор HTTP-профилей (`build_request_headers`) — спуфит порядок и состав заголовков под Chrome/Firefox/Edge/Tor Browser (`Accept`, `Accept-Language`, `Sec-Fetch-*` и т.д., см. doc-comment в шапке файла, пункты 1-11). На уровне HTTP это выглядит браузерно.

Но `curl` с той же UA-строкой (и заведомо не подделанными остальными заголовками/TLS) проходит, а Lumen — нет. Это указывает не на HTTP-заголовки, а на что-то ниже: TLS ClientHello Lumen (`crates/network/src/tls/fingerprint.rs`) — порядок cipher suite, набор extensions, GREASE-значения, ALPN — вероятно, не совпадает с профилем реального браузера настолько, чтобы анти-бот Cloudflare (JA3/JA4-скоринг) его пропустил, хотя по HTTP-заголовкам Lumen выглядит легитимно.

GitHub (тоже за CDN-анти-бот) грузится нормально — значит проблема не универсальна для всех защищённых сайтов, а зависит от конкретного порога/провайдера бот-детекта (httpbin.org, судя по всему, использует более строгую эвристику, либо там дополнительно сказывается независимая деградация самого демо-сервиса — не исключено на 100%, но 4/4 стабильных 503 против 100% успеха curl с той же UA делают чисто сетевую деградацию httpbin маловероятной единственной причиной).

## Потенциальное влияние

Любой сайт с сопоставимо строгим анти-бот-порогом (Cloudflare Bot Management, аналоги) может молча отдавать Lumen 503/капчу, даже когда страница публично доступна из любого мейнстрим-браузера — это функциональный и в чём-то приватностный пробел: Lumen явно реализует HTTP-фингерпринт-спуфинг (`tls/fingerprint.rs`, `http/headers.rs`), но он, похоже, не держится на TLS-уровне.

## Что нужно для локализации

Сравнить реальные байты TLS ClientHello, которые шлёт Lumen (`tls/fingerprint.rs`), с ClientHello настоящего Chrome/Firefox и с `curl` (например, через `tshark`/`Wireshark` или логированием `rustls::ClientConfig` на стороне Lumen) — порядок cipher suites, extensions, ALPN-протоколы, GREASE. Возможно, потребуется JA3/JA4-специфичная донастройка `rustls::ClientConfig`, которая на данный момент не входит в `tls/fingerprint.rs`.

## Репро

```bash
target/dev-release/lumen.exe --dump-source https://httpbin.org/forms/post   # 503, 4/4 раз
curl -A "Lumen/0.5.0" https://httpbin.org/forms/post                        # 200 каждый раз
target/dev-release/lumen.exe --dump-source https://github.com/              # 200 — сеть в целом жива
```
