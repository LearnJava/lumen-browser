# RP-7 — Устойчивость к анти-бот защите (403 от Cloudflare/подобных)

**Developer:** P1 (расследование) · **Branch:** `p1-rp7-antibot` · **Size:** M (сначала диагностика, потом решение) · **Crates:** `lumen-network`, возможно `lumen-shell`
**Roadmap:** RP-7 (под RP, P3)

---

## Статус

**OPEN / расследование.** Аудит 2026-07-02: из 14 сайтов **4 не открылись на уровне HTTP** — `stackoverflow.com` (403), `crates.io` (403), `ria.ru` (403), `docs.rs` (500). Edge открывает все. Страницы физически нет — это не рендер-баг.

## Что уже сделано (и почему 403 удивителен)

Сеть УЖЕ маскируется под Chrome 130:
- TLS: rustls + aws-lc-rs, cipher-order/named-groups/ALPN под Chrome 130, снапшоты `CHROME_130_JA3/JA4` (`crates/network/src/tls/mod.rs:73-135`, `tls/fingerprint.rs`).
- Заголовки: Chrome-порядок + значения (`crates/network/src/http/headers.rs:125-147`) — `User-Agent` Chrome/130, `Accept`, `Accept-Language`, `Sec-Fetch-*`, `DNT` и т.д.
- HTTP/2 реально работает (ALPN `h2` → `h2_do_request`, см. `h2/`).

То есть простой JA3/UA-фильтр мы уже проходим. 403 приходит, скорее всего, от более умной защиты.

## Гипотезы (проверить по порядку)

1. **`Accept: */*` на top-level navigate.** Реальный Chrome шлёт для документа `Accept: text/html,application/xhtml+xml,application/xml;q=0.9,image/avif,image/webp,image/apng,*/*;q=0.8,...`. У нас (`headers.rs:~137`) для Chrome-профиля `Accept: */*` — это НЕ то, что шлёт браузер на навигацию, и легко палится. **Начать отсюда** — дёшево и вероятно.
2. **JA3/JA4 дрейф.** Снапшот `CHROME_130_*` мог устареть или rustls опускает CBC-сюиты, которые Chrome включает → hash не совпадает. Снять реальный JA3 Lumen (напр. через ja3er/tls.peet.ws аналог локально) и сверить с живым Chrome.
3. **HTTP/2-специфика.** Порядок псевдо-заголовков (`:method/:scheme/:authority/:path`), SETTINGS-кадр, приоритеты — у Chrome характерный «Akamai HTTP/2 fingerprint». rustls-H2 у нас свой — может отличаться. Проверить, ловится ли по нему.
4. **Cloudflare JS-challenge.** stackoverflow/crates.io за Cloudflare — могут требовать исполнить JS-challenge (`cf_clearance` cookie). Это принципиально сложнее: нужен рабочий JS-движок на challenge-странице + cookie-jar. Тут пересекается с JS-perf/V8 (challenge на QuickJS может не пройти по таймингу).
5. **`docs.rs` 500** — отдельно: возможно, наш HTTP/2 или заголовок ломает именно их бэкенд. Попробовать форсить HTTP/1.1 к docs.rs и сравнить.

## Что сделать (поэтапно)

1. **Диагностика:** прогнать 4 сайта с трейсом заголовков запроса/ответа (есть `network_log`), снять реальный JA3/JA4 Lumen, сравнить с Chrome. Зафиксировать, какой именно слой режет (TLS handshake? первый ответ? challenge-редирект?).
2. **Дешёвые фиксы:** привести `Accept` навигации к Chrome-виду (гипотеза 1); синхронизировать JA3-снапшот (гипотеза 2).
3. **Решение по challenge:** если дело в Cloudflare JS-challenge — это большая связка с cookie-jar + JS; вынести в отдельную задачу, не смешивать.

## Замечание

Не превращать в «гонку вооружений» с анти-ботами ради неё самой. Приоритет — массовые ложные срабатывания на обычном контенте (SO/crates.io — сайты, куда пользователь реально ходит). Цель — вести себя как настоящий Chrome на уровне сети, а не эмулировать обход капчи.
