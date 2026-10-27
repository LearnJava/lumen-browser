# BUG-1739 — SubtleCrypto игнорирует параметры операций (`tagLength`, `saltLength`, `length`) и копирует данные после нормализации алгоритма

**Статус:** OPEN
**Заведён:** 2026-10-27 (P2, WPT-CRYPTO-SCOUT)
**Область:** js (`crates/js/src/shim/crypto_shim.js` — `encrypt`/`decrypt`/`sign`/`verify`/`digest`/`deriveBits`; `crates/js/src/subtle_crypto.rs` — `aes_gcm_*`, `rsa_verify`, `aes_ctr_crypt`, HKDF/PBKDF2)

## Симптом

Операции на поддержанных алгоритмах дают неверный результат:

- AES-GCM: `tagLength` не передаётся в натив, тег всегда 128 бит. Векторы с 32…120-битным тегом дают не тот шифртекст («Should return expected result»), а decrypt падает (`encrypt_decrypt/aes_gcm`, `aes_gcm_256_iv`);
- RSA-PSS: `rsa_verify` не читает `saltLength` (`_alg_json`), поэтому подпись «no salt» не проходит проверку (`sign_verify/rsa_pss`, «Signature verified expected true got false»); у RSASSA-PKCS1/HMAC/ECDSA с SHA-1 — то же из-за хеша по умолчанию (см. BUG-1736);
- AES-CTR: векторы `encrypt_decrypt/aes_ctr` (32 сабтеста) дают не тот результат: проверить `counter`/`length`;
- `deriveBits` HKDF/PBKDF2 с `length` 0 или не кратным 8: ожидается пустой буфер / `OperationError`, а шим подставляет `length || 256` (`derive_bits_keys/derived_bits_length`); у `deriveKey` → HMAC без `length` длина берётся неверно («Exported key matches correct value expected true got false», `pbkdf2`/`hkdf`);
- «… with transferred buffer during call» (`digest`, `sign_verify/*`, `encrypt_decrypt/*`): тест отсоединяет буфер данных из геттера словаря алгоритма. Шим сначала делает `JSON.stringify(algorithm)` (геттер срабатывает) и только потом читает данные, отсюда «Cannot perform Construct on a detached ArrayBuffer». По спецификации байты данных копируются до нормализации алгоритма.

## Как найдено

WPT-CRYPTO-SCOUT, кластеры G и F: 35 + 25 id, потолок 4.7 + 3.0 балла (2.5 п.п. категории). Перечень сообщений — `docs/wpt-vendor-notes/WebCryptoAPI.md`.

## Что делать

Передавать в натив все параметры словаря (`tagLength`, `saltLength`, `counter`+`length`, `length` для `deriveBits`/`deriveKey`); копию байтов данных брать первым шагом метода; длину HMAC-ключа в `deriveKey` брать из «get key length» алгоритма. Удобнее после BUG-1736 (нормализованные словари параметров).

## Как проверить

`run_corpus.py --prefixes WebCryptoAPI/encrypt_decrypt WebCryptoAPI/sign_verify WebCryptoAPI/digest/digest`: исчезают «Should return expected result», «Signature verified expected true got false» по поддержанным алгоритмам и «detached ArrayBuffer».
