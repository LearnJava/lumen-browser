# BUG-1737 — `structuredClone`/`postMessage` теряют `CryptoKey`: клон — простой объект без ключа

**Статус:** OPEN
**Заведён:** 2026-10-27 (P2, WPT-CRYPTO-SCOUT)
**Область:** js (structured clone — `crates/js/src/v8_runtime/` (сериализатор V8), `crates/js/src/shim/crypto_shim.js` — `CryptoKey.__ckid`)

## Симптом

```js
const k = await crypto.subtle.generateKey({name: "AES-GCM", length: 128}, true, ["encrypt"]);
const {key} = structuredClone({key: k});
key instanceof CryptoKey;                 // false
await crypto.subtle.exportKey("raw", key); // TypeError: exportKey: argument is not a CryptoKey
```

`CryptoKey` — обычный JS-объект шима с `__ckid`. Клон получает собственные поля, но прототипа и связи с ключом в `CRYPTO_KEYS` (`subtle_crypto.rs`) у него нет. По спецификации `CryptoKey` — `[Serializable]`: клон ссылается на тот же материал ключа, а `[[extractable]]`, `[[usages]]` и `[[algorithm]]` сохраняются.

## Как найдено

WPT-CRYPTO-SCOUT, кластер D: `serialization/*.https.any*.html`, все 38 id 0/126 (`serialization/serialization.js:10`, `structuredClone({key: cryptoKey})`). Для поддержанных алгоритмов (AES-CBC/CTR/GCM, ECDH/ECDSA P-256, HMAC, RSA-*) это 18 id, потолок ≈17 баллов. Остальные 20 id дополнительно ждут алгоритмов из `WEBCRYPTO-CURVES`/`-SYM`/`-PQ`.

## Что делать

Сделать `CryptoKey` сериализуемым платформенным объектом: в host-хуках сериализатора V8 (`WriteHostObject`/`ReadHostObject` или их аналог для прочих `[Serializable]` в `crates/js`) писать id ключа, а при десериализации создавать новый `CryptoKey` на тот же материал. Внутри одного процесса материал общий, а для воркеров (`postMessage` в `worker.rs`) материал нужно копировать в хранилище изолята-получателя: `CRYPTO_KEYS` — `thread_local`.

## Как проверить

Проба выше; `run_corpus.py --prefixes WebCryptoAPI/serialization` — 18 id поддержанных алгоритмов PASS.
