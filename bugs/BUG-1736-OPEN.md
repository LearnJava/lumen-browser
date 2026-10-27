# BUG-1736 — SubtleCrypto без «normalize an algorithm» и проверок usages: не те исключения и успех там, где нужна ошибка

**Статус:** OPEN
**Заведён:** 2026-10-27 (P2, WPT-CRYPTO-SCOUT)
**Область:** js (`crates/js/src/shim/crypto_shim.js` — все методы `subtle`; `crates/js/src/subtle_crypto.rs` — `generate_key`, `import_key`, `json_str_field(alg_json, "hash")`)

## Симптом

Шим передаёт в Rust `JSON.stringify(algorithm)` как есть, а Rust сопоставляет имя через `to_ascii_uppercase()`. Нормализации алгоритма (WebCrypto §18.4) нет, проверок usages и типа ключа тоже. Поэтому:

```js
await crypto.subtle.generateKey({name: "AES-CTR", length: 192}, true, ["sign"]);   // NotSupportedError, ожидается SyntaxError (недопустимые usages)
await crypto.subtle.generateKey({name: "AES-GCM", length: 128}, true, []);         // ключ создан, ожидается SyntaxError (пустые usages у секретного ключа)
await crypto.subtle.generateKey({}, false, ["sign"]);                              // NotSupportedError, ожидается TypeError (нет name)
await crypto.subtle.generateKey({name: "HMAC", hash: "MD5"}, false, ["sign"]);     // ключ создан, ожидается NotSupportedError
await crypto.subtle.generateKey({name: "AES-CTR", length: 64}, false, ["encrypt"]); // NotSupportedError, ожидается OperationError
await crypto.subtle.deriveKey(hkdfParams, keyWithoutDeriveKeyUsage, {name: "AES-GCM", length: 256}, true, ["encrypt"]); // успех, ожидается InvalidAccessError
await crypto.subtle.deriveBits({name: "HKDF", hash: "PBKDF2", …}, key, 256);      // успех, ожидается NotSupportedError (хеш не digest-алгоритм)
await crypto.subtle.encrypt({name: "RSA-OAEP"}, rsaPrivateKey, data);             // OperationError, ожидается InvalidAccessError (usage/тип ключа)
await crypto.subtle.deriveBits({name: "HKDF", …}, key, 256);                  // OperationError, ожидается NotSupportedError (имя сравнивается ASCII-регистронезависимо, K-знак Кельвина не совпадает)
```

Хеш-параметр `{hash: {name: "SHA-1"}}` (словарь, а не строка) `json_str_field` не находит и молча берёт SHA-256 (часть «Signature verified expected true got false» у HMAC/ECDSA SHA-1).

## Как найдено

WPT-CRYPTO-SCOUT, кластер B: 107 id, 23 274 сабтеста, потолок 39.6 балла (12.6 п.п. категории): `generateKey/failures_*` («Bad usages», «Empty usages», «Bad algorithm», «Empty algorithm», «Bad algorithm property»), `import_export/*_importKey_failures_*`, `derive_bits_keys/hkdf`/`pbkdf2` («wrong (ECDH) key», «bad hash name», «missing deriveKey usage», «0 iterations», «non-multiple of 8 length»), `encrypt_decrypt/rsa_oaep`, `sign_verify/*` («using privateKey to verify», «no verify usage»), `normalize-algorithm-name`. Разбор — `docs/wpt-vendor-notes/WebCryptoAPI.md`.

## Что делать

1. В шиме — таблица «операция → алгоритм → тип словаря параметров» (§18.4.4) и `normalizeAlgorithm(op, alg)`: строка → `{name}`; `name` обязателен (`TypeError`); поиск ASCII-регистронезависимый (без Unicode case folding), неизвестное → `NotSupportedError`; поля словаря приводятся по WebIDL (BufferSource копируется, `hash` нормализуется рекурсивно как `digest`).
2. Шаги каждого метода по спецификации в том же порядке: нормализация → проверка `[[usages]]` ключа и совпадения `[[algorithm]].name` (`InvalidAccessError`) → проверки, свойственные алгоритму (usages при генерации/импорте → `SyntaxError`, пустые usages секретного/приватного ключа → `SyntaxError`, длина → `OperationError`, данные ключа → `DataError`).
3. В Rust передавать уже нормализованный JSON с каноническим именем и `hash` строкой.

Фиксы BUG-1734 и BUG-1739 удобно делать после этого: обоим нужна та же таблица алгоритмов.

## Как проверить

Проба выше; `run_corpus.py --prefixes WebCryptoAPI/generateKey WebCryptoAPI/import_export WebCryptoAPI/derive_bits_keys`: падения «expected "SyntaxError" but got "NotSupportedError"», «Operation succeeded, but should not have», «should have thrown» по поддержанным алгоритмам исчезают.
