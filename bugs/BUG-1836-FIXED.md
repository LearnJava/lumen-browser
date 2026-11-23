# BUG-1836 — TLS ClientHello отличался от Chrome 157 (шифры `c009,c013` вместо `c013,c014`, нет ML-DSA в `signature_algorithms`) — okko.tv выдаёт js-challenge

**Статус:** FIXED 2026-11-23 — манифест `CHROME_157.tls` приведён к измеренному Chrome 157; okko.tv отдаёт приложение сразу, js-challenge нет (в том числе с честным токеном `Lumen`).
**Заведён:** 2026-11-23 (ZCode, задача «okko.tv: сравнить с Chrome и ускорить»)
**Компонент:** network ([`crates/network/src/http/chrome_manifest.rs`](../crates/network/src/http/chrome_manifest.rs), [`crates/network/src/tls/boring.rs`](../crates/network/src/tls/boring.rs))

## Симптом

`https://okko.tv/` (антибот ServicePipe) в Lumen: документ — js-challenge (1813 байт, спиннер +
`servicepipe.tech/static/checkjs/*.js`), скрипт ~9.5 с опрашивает `okko.tv/api/v1/antibot/check/get-func/<id>`
(110 запросов за прогон), и только затем грузится приложение. Chrome 157 получает `307` → приложение сразу.

- Lumen до правки (1 прогон): challenge ушёл на 11.8 с, приложение готово на 29.1 с. Chrome: 4.27 с.
- Lumen после правки (3 прогона, медианы): `DOMContentLoaded` 828 мс, первый кадр 2573 мс, `load` 7868 мс.
  Chrome 157.0.8103.79 (3 прогона, медианы): 731 / 1352 / 2787 мс.

## Что решает сервер

Матрица клиентов напрямую, без cookie (`.tmp/okko/tools/wire.py`, curl_cffi 0.17.0, 2026-11-23):

| Клиент | Ответ `GET https://okko.tv/` |
|---|---|
| TLS-профиль актуального Chrome, HTTP/2 | `302` → `auth.okko.sport/v1/sso`, без challenge |
| тот же TLS-профиль, HTTP/1.1 | `302`, без challenge |
| TLS-профиль Chrome 131, HTTP/2 | `200`, js-challenge |
| TLS-профиль Chrome 131, HTTP/1.1 | `200`, js-challenge |
| обычный curl TLS | `403` |

Сервер ветвит по TLS-отпечатку; версия HTTP (1.1 против 2) в этой матрице роли не сыграла.

## Дельта ClientHello (tls.peet.ws/api/all, навигация документа, напрямую, 2026-11-23)

| | Lumen до | Chrome 157.0.8103.79 | Lumen после |
|---|---|---|---|
| JA4 | `t13d1518h2_e2d637829cee_f9a770a73cc2` | `t13d1517h2_8daaf6152771_cb7bf5808d99` | `t13d1518h2_8daaf6152771_4980c97edce0` |
| шифры TLS 1.2 | `…c009,c013,…` | `…c013,c014,…` | `…c013,c014,…` — совпало |
| `signature_algorithms` | без PQ-схем | `GREASE,2308,2309,2310,1027,…` (ML-DSA-44/65/87 первыми) | совпало |
| расширения | 20 типов, лишнее `4832` | 19 | 20, лишнее `4832` |
| Akamai HTTP/2 | `1:65536;2:0;4:6291456;6:262144\|15663105\|0\|m,a,s,p` | то же | то же |

Причина в манифесте: в `cipher_list` стояли `ECDHE-ECDSA-AES128-SHA:ECDHE-RSA-AES128-SHA` (`c009,c013`),
а Chrome шлёт `ECDHE-RSA-AES128-SHA:ECDHE-RSA-AES256-SHA` (`c013,c014`); в `sigalgs` не было ML-DSA.
`set_cipher_list`/`set_sigalgs_list` ошибку не давали — имя валидно, просто не то.

## Исправление

- `chrome_manifest.rs`: `cipher_list` → `ECDHE-RSA-AES128-SHA:ECDHE-RSA-AES256-SHA`; `sigalgs` с префиксом
  `mldsa44:mldsa65:mldsa87:` (BoringSSL 5.5.1 знает эти имена).
- Тест `tls_cipher_list_is_measured_chrome_157` читает TLS 1.2-половину списка обратно из BoringSSL
  (`SslContextRef::ciphers()` + `protocol_id()`) и сверяет с измеренным порядком; `tls_lists_are_accepted_by_boringssl`
  требует, чтобы все три списка принимались стеком.
- `boring.rs`: ошибка `set_cipher_list`/`set_sigalgs_list` теперь отказ соединения, а не молчаливый дефолт стека.

**Проверка:** `lumen --dump-source https://okko.tv/` (без прокси, холодный профиль) — с правкой
`302` → приложение (437 622 байта) и с `chrome_lumen`, и с `chrome_unbranded`; прежний бинарь — `200`, 1812 байт
(challenge). Токен `Lumen/x.y` в User-Agent и бренд `"Lumen"` в `sec-ch-ua` challenge на этом сайте не вызывают.
`cargo test -p lumen-network --lib chrome_manifest` 6/6, `cargo clippy -p lumen-network --all-targets -- -D warnings` чисто.

## Остаток — расширение 4832 (`server_padding`), не закрыто

`boring.rs` добавляет его с BUG-1835 («Chrome шлёт»). Замеры расходятся: Chrome for Testing 157.0.8103.78
от 2026-11-20 (`docs/perf/runs/2026-11-20-peet-chrome.json`) расширение отправляет, стабильный Chrome
157.0.8103.79 на этой машине 2026-11-23 — нет. okko.tv принимает оба варианта (Lumen с `4832` challenge не получает).
Какой вариант у большинства реальных Chrome — **не проверено**; пока `4832` оставлен (вариант CfT — эталон
`scripts/fingerprint_selfcheck.py`). Проверка: снять peet-отпечаток стабильного Chrome ещё на 2–3 машинах.
