# BUG-1728 — `Crypto`/`SubtleCrypto`/`CryptoKey` не WebIDL-интерфейсы: нет глобала `SubtleCrypto`, методы — own-свойства; `getRandomValues` пропускает Float16Array/DataView

**Статус:** OPEN
**Заведён:** 2026-10-27 (P2, WPT-CRYPTO-SCOUT)
**Область:** js (`crates/js/src/shim/crypto_shim.js` — `subtle`, `g.crypto`, `g.Crypto`, `getRandomValues`)

## Симптом

```js
typeof SubtleCrypto;                              // "undefined"
Object.getOwnPropertyNames(crypto.subtle);        // ["digest","generateKey",…]: методы на объекте, не на прототипе
Object.getOwnPropertyDescriptor(self, "Crypto").enumerable;   // true, ожидается false
crypto.getRandomValues(new Float16Array(6));      // не бросает, ожидается TypeMismatchError
crypto.getRandomValues(new Uint8Array(65537));    // DOMException "QuotaExceededError", тест ждёт интерфейс QuotaExceededError
```

`crypto.subtle` — литерал-объект, `crypto` — литерал с двумя методами, а `Crypto` — пустая функция.

## Как найдено

WPT-CRYPTO-SCOUT, кластер A2: `idlharness.https.any*.html` (4 id, 90/380), а `supports*.tentative` падают с `ReferenceError: SubtleCrypto is not defined` (сам `SubtleCrypto.supports` — фича, см. `WEBCRYPTO-PQ`). `getRandomValues.any*.html`: 54/78. Потолок ≈7.6 балла (2.4 п.п. категории).

## Что делать

1. `Crypto`, `SubtleCrypto`, `CryptoKey` — интерфейсы с прототипами по WebIDL (`Exposed=(Window,Worker)`, `[SecureContext]` у `SubtleCrypto`/`CryptoKey`), не-перечислимые глобалы, `new` бросает `TypeError` (Illegal constructor); `crypto.subtle` — геттер `[SameObject]` на `Crypto.prototype`; методы — на `SubtleCrypto.prototype`. Проверить, что разделение `exposeSecure()` для воркеров (BUG-1688) сохраняется.
2. `getRandomValues`: `TypeMismatchError` для `Float16Array`/`Float32Array`/`Float64Array`/`DataView`; переполнение — `QuotaExceededError` в той форме, которую ждёт `assert_throws_quotaexceedederror` (`resources/testharness.js`).

## Как проверить

`run_corpus.py --prefixes WebCryptoAPI/idlharness WebCryptoAPI/getRandomValues` — 100 % (кроме `historical`, см. заметку категории: на `localhost` контекст безопасный).
