# BUG-331: Cloudflare-защищённые сайты возвращают `HTTP 503` для Lumen, хотя HTTP-заголовки корректно подделаны под браузер

**Статус:** FIXED (не воспроизводится) 2026-08-06
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

## Ревизия 2026-08-06 (P3): не воспроизводится, закрыт

**Код `tls/`/`http/headers.rs` не менялся ни разу с момента подачи заявки** (`git log --since=2026-07-23 -- crates/network/src/tls/ crates/network/src/http/headers.rs` — пусто), поэтому любое изменение поведения объясняется не фиксом Lumen, а состоянием на стороне httpbin.org/Cloudflare в момент первой заявки.

Живая перепроверка на свежей `dev-release`-сборке (`p3-work` worktree, HEAD на момент проверки): **9 из 9** запросов `--dump-source https://httpbin.org/forms/post` вернули `200` подряд (два прогона по 4 и 5 попыток). Исходный симптом (4/4 `503`) не воспроизводится.

Разобран текущий `crates/network/src/tls/mod.rs::build_client_config` — TLS ClientHello уже приближен к Chrome 130: cipher suites в порядке Chrome (TLS 1.3 AEAD → TLS 1.2 ECDHE AEAD), `kx_groups` X25519→secp256r1→secp384r1, ALPN h2+http/1.1. Не хватает GREASE-инъекции и точного Chrome-порядка расширений — это упирается в возможности `rustls` (не даёт контролировать порядок extensions/добавлять GREASE-значения без замены TLS-стека на что-то вроде BoringSSL-биндингов) и осталось бы систематическим ограничением, но раз исходный симптом не воспроизводится, нет живого сигнала подтвердить или опровергнуть, что TLS-фингерпринт вообще был истинной причиной прежних `503` — гипотеза остаётся недоказанной.

**Побочная находка при живой проверке**: `https://www.cloudflare.com/` (другой CDN-защищённый сайт) стабильно падал с `network error: HTTP 103` — сервер шлёт `103 Early Hints` (RFC 8297) перед финальным `200` по HTTP/2, а Lumen трактовал промежуточный статус как терминальный. Это отдельный, реальный и подтверждённый дефект (не связан с TLS-фингерпринтом/JA3) — заведён и исправлен отдельно как [BUG-679](BUG-679-FIXED.md).

**Итог:** закрыт как «не воспроизводится» — точечного P3-дефекта в TLS/HTTP-заголовках для исходного репро не найдено; если проблема вернётся на httpbin.org или другом Cloudflare-сайте, переоткрыть с новым живым захватом ClientHello (`tshark`/`Wireshark`) для реальной JA3/JA4-локализации.
