# Задача: Web Crypto SubtleCrypto — полный набор алгоритмов

**Developer:** P1
**Ветка:** `p1-webcrypto`
**Размер:** M
**Крейты:** `lumen-js`

## Goal
Дошить `crypto.subtle` до практически полного набора алгоритмов из W3C Web
Cryptography API §14: RSA (RSASSA-PKCS1-v1_5 / RSA-PSS / RSA-OAEP), Ed25519,
PBKDF2 + HKDF (deriveBits/deriveKey), AES-CBC/AES-CTR (encrypt/decrypt), а также
привести standalone `digest` в общий стиль. Сейчас поддержаны только ECDSA-P256,
HMAC-SHA*, AES-GCM.

## Current state (сверено с кодом 2026-07-05)
- Rust-ядро: `crates/js/src/subtle_crypto.rs:1-1020`. Заголовок-таблица
  поддержки — `subtle_crypto.rs:6-12` (ECDSA P-256 / HMAC-SHA* / AES-GCM).
- Реестр ключей `KeyMaterial` — `subtle_crypto.rs:33-43`: варианты `Hmac`,
  `EcdsaPrivate`, `EcdsaPublic`, `AesGcm`. **Нет** RSA/Ed25519/derived-bits.
- `generate_key` — `subtle_crypto.rs:154`, match по `alg.name`: ветки `"HMAC"`
  (:157), `"ECDSA"` (:181), `"AES-GCM"` (:215). Неизвестный алгоритм → err
  (тест `generate_key_unsupported_algo_returns_err` на `RSA-OAEP`,
  `subtle_crypto.rs:1015`).
- `import_key` — `:244` (HMAC :253 / ECDSA :280 / AES-GCM :407);
  `export_key` — `:442`; `sign_data` — `:515`; `verify_signature` — `:561`;
  `aes_gcm_encrypt`/`decrypt` — `:617`/`:658`.
- JS-биндинги регистрируются в `install_subtle_bindings` —
  `subtle_crypto.rs:716` (`_lumen_subtle_generate_key`, `_import_key`,
  `_export_key(_or_err)`, `_sign`, `_verify`, `_encrypt`, `_decrypt`,
  `_key_info`). **Encrypt/decrypt жёстко завязаны на AES-GCM** (аргументы
  `iv, aad` — `:778`/`:784`).
- JS-shim `crypto.subtle` — `crates/js/src/dom.rs:11271-11447`: `digest`
  (:11273, зовёт `_lumen_sha_digest`), `generateKey` (:11292),
  `importKey` (:11314), `exportKey` (:11341), `sign` (:11368),
  `verify` (:11387), `encrypt` (:11403), `decrypt` (:11422).
- **Заглушки:** `wrapKey`/`unwrapKey`/`deriveBits`/`deriveKey` возвращают
  `Promise.reject(NotSupportedError)` — `dom.rs:11431-11443`.
- Standalone digest `_lumen_sha_digest` (SHA-1/256/384/512) —
  `dom.rs:2846-2858`. Работает, но живёт вне `subtle_crypto.rs`.

## Entry points
- `crates/js/src/subtle_crypto.rs:33` — enum `KeyMaterial` (добавлять варианты).
- `crates/js/src/subtle_crypto.rs:154` / `:244` / `:442` — generate/import/export
  (расширять match по `alg.name`).
- `crates/js/src/subtle_crypto.rs:515` / `:561` — sign/verify (RSA/Ed25519).
- `crates/js/src/subtle_crypto.rs:716` — регистрация JS-биндингов (добавить
  `_lumen_subtle_derive_bits`, обобщить encrypt/decrypt по алгоритму).
- `crates/js/src/dom.rs:11271` — JS-shim subtle (диспетчер по `algorithm.name`).
- `crates/js/src/dom.rs:11431` — заглушки deriveBits/deriveKey (заменить).

## Срезы (декомпозиция)
### Срез 1 — S — AES-CBC + AES-CTR (encrypt/decrypt)
Добавить `KeyMaterial::AesCbc(Vec<u8>)` / `AesCtr(Vec<u8>)` (или переиспользовать
`AesGcm` с меткой режима). Ветки generate/import/export по аналогии с AES-GCM
(`subtle_crypto.rs:215/407`). Обобщить `_lumen_subtle_encrypt/decrypt` — либо
новые биндинги `_lumen_subtle_aes_cbc_*`/`_ctr_*`, либо передавать `alg_json`.
JS-shim `encrypt`/`decrypt` (`dom.rs:11403/11422`) — разбор `iv`/`counter` по
имени алгоритма. Крейты: `aes`, `cbc`, `ctr` (или `aes-gcm`-совместимые).
Обосновать новые deps в теле коммита.

### Срез 2 — S — standalone digest в subtle_crypto.rs
Перенести/задублировать логику `_lumen_sha_digest` (`dom.rs:2846`) в
`subtle_crypto.rs` как `pub(crate) fn digest(algo, data)` + юнит-тесты на
известные вектора (пустой вход SHA-256/SHA-1). JS-shim `digest`
(`dom.rs:11273`) не меняем по поведению, только источник функции. Это чистка,
чтобы весь Web Crypto жил в одном модуле.

### Срез 3 — S — PBKDF2 deriveBits/deriveKey
`KeyMaterial::Pbkdf2 { raw: Vec<u8> }` (импорт `raw` пароля, extractable=false).
Новый биндинг `_lumen_subtle_derive_bits(alg_json, key_id, length)` в
`install_subtle_bindings` (`subtle_crypto.rs:716`). Разбор `salt`/`iterations`/
`hash` из `alg_json`. JS: заменить заглушку `deriveBits` (`dom.rs:11438`) и
реализовать `deriveKey` как deriveBits → importKey. Крейт `pbkdf2`.

### Срез 4 — S — HKDF deriveBits/deriveKey
`KeyMaterial::Hkdf { raw }` + ветка в том же `_lumen_subtle_derive_bits`
(диспетч по `alg.name` = "HKDF"): разбор `salt`/`info`/`hash`. Крейт `hkdf`.
Покрыть тестом на вектор RFC 5869.

### Срез 5 — M — RSA (RSASSA-PKCS1-v1_5 / RSA-PSS / RSA-OAEP)
`KeyMaterial::RsaPrivate`/`RsaPublic` (крейт `rsa`). generateKey с
`modulusLength`/`publicExponent` (`subtle_crypto.rs:154`), import/export
`spki`/`pkcs8`/`jwk` (`:244`/`:442`), sign/verify для PKCS1-v1_5 и PSS
(`:515`/`:561`), encrypt/decrypt для RSA-OAEP. Самый крупный срез — RSA-генерация
медленная, тесты гонять на импортированных фикстурах, а не generateKey.

### Срез 6 — S — Ed25519 (sign/verify/generate/import/export)
`KeyMaterial::Ed25519Private`/`Public` (крейт `ed25519-dalek`). generate,
import/export `raw`/`pkcs8`/`spki`/`jwk`, sign/verify. По структуре аналогичен
ECDSA (`subtle_crypto.rs:181/280`).

## Tests
- Rust-юниты в `subtle_crypto.rs` (модуль `tests`, `:801`): roundtrip
  generate→sign→verify и encrypt→decrypt на каждый новый алгоритм; import/export
  фикстур; PBKDF2/HKDF против известных векторов RFC.
- JS-интеграция в `dom.rs` (рядом с `crypto_subtle_*`, `:20364+`): промисные
  тесты `generateKey`/`deriveBits` резолвятся, `encrypt(AES-CBC)` →
  `decrypt` даёт исходный текст, `deriveBits(PBKDF2)` даёт нужную длину.
- Негативные: неизвестный алгоритм → `NotSupportedError`; порча ciphertext AES →
  reject.

## Definition of done
- [ ] AES-CBC и AES-CTR: encrypt/decrypt/generateKey/importKey/exportKey.
- [ ] PBKDF2 deriveBits/deriveKey (заглушка `dom.rs:11438` убрана).
- [ ] HKDF deriveBits/deriveKey.
- [ ] RSA: PKCS1-v1_5, PSS (sign/verify), OAEP (encrypt/decrypt), gen/import/export.
- [ ] Ed25519: sign/verify/generate/import/export.
- [ ] digest перенесён в `subtle_crypto.rs`.
- [ ] Таблица поддержки в шапке `subtle_crypto.rs:6-12` обновлена.
- [ ] Новые крейты обоснованы в теле коммита (`docs/plan/tech-stack.md` §5).
- [ ] `CAPABILITIES.md` — Web Crypto ⬜/🟡 → ✅ по мере готовности.
