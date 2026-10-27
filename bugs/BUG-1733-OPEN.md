# BUG-1733 — `CryptoKey`: нет `Symbol.toStringTag`, у `algorithm` нет `length`, имя RSASSA в верхнем регистре, неверные `usages`/`extractable`

**Статус:** OPEN
**Заведён:** 2026-10-27 (P2, WPT-CRYPTO-SCOUT)
**Область:** js (`crates/js/src/shim/crypto_shim.js` — `CryptoKey`, `_make_crypto_key`; `crates/js/src/subtle_crypto.rs` — `generate_key`, `import_key`, `algorithm_json` ключа)

## Симптом

```js
const k = await crypto.subtle.generateKey({name: "AES-GCM", length: 128}, true, ["encrypt"]);
Object.prototype.toString.call(k);   // "[object Object]", ожидается "[object CryptoKey]"
k.algorithm.length;                  // undefined, ожидается 128 (то же у HMAC)
const r = await crypto.subtle.importKey("spki", spki, {name: "RSASSA-PKCS1-v1_5", hash: "SHA-256"}, true, ["verify"]);
r.algorithm.name;                    // "RSASSA-PKCS1-V1_5", ожидается "RSASSA-PKCS1-v1_5"
```

Ещё два расхождения той же проверки `checkKeyAlgorithm`/`assert_goodCryptoKey` (`tests/wpt/WebCryptoAPI/util/helpers.js`):
- `generateKey` RSA-OAEP с `["decrypt","encrypt","unwrapKey","wrapKey"]`: у `privateKey.usages` нет `decrypt` («Has decrypt usage value "decrypt" not in array ["unwrapKey"]»): usages пары делятся не по §22 (public — `encrypt`/`wrapKey`/`verify`, private — `decrypt`/`unwrapKey`/`sign`);
- `importKey("spki", …, extractable=false)` для ECDSA даёт `extractable === true` («Extractability is correct expected false»). Публичный ключ всегда extractable только в `generateKey`, а в `importKey` флаг берётся из аргумента.

## Как найдено

WPT-CRYPTO-SCOUT, кластер A: 66 id, 4 684 сабтеста, потолок 58.7 балла (18.8 п.п. категории). Первым падает `assert_goodCryptoKey`, поэтому все `generateKey/successes_*` и `import_export/*_importKey` «Good parameters» по поддержанным алгоритмам стоят в FAIL: `successes_RSA-OAEP` (32 id, ≈32 балла), `successes_RSA-PSS`/`RSASSA-PKCS1-v1_5` (по 8 id), `successes_AES-*`/`HMAC`/`ECDSA`, `rsa_importKey`, `symmetric_importKey`, `ec_importKey`.

## Что делать

1. `CryptoKey.prototype[Symbol.toStringTag] = "CryptoKey"`, `type`/`extractable`/`algorithm`/`usages` — геттеры прототипа (кэшированный объект `algorithm`, как сейчас проходит `crypto_key_cached_slots`).
2. `algorithm` ключа строить по словарям `*KeyAlgorithm` спецификации: `AesKeyAlgorithm {name, length}`, `HmacKeyAlgorithm {name, hash: {name}, length}`, `RsaHashedKeyAlgorithm {name, modulusLength, publicExponent, hash}`, `EcKeyAlgorithm {name, namedCurve}`. Имя — каноническое регистровое из таблицы алгоритмов, а не `to_ascii_uppercase()`.
3. Разделение usages пары и `extractable` при импорте — по шагам `generate key`/`import key` каждого алгоритма.

## Как проверить

Проба выше; `run_corpus.py --prefixes WebCryptoAPI`: сообщения «Symbol.toStringTag», «Correct length», «Correct algorithm name» исчезают. Падения следующих проверок тех же сабтестов останутся и относятся к другим кластерам.
