# BUG-1737 — SubtleCrypto считает синхронно в executor промиса: PBKDF2/RSA-keygen/большие данные блокируют загрузку страницы

**Статус:** OPEN
**Заведён:** 2026-10-27 (P2, WPT-CRYPTO-SCOUT)
**Область:** js (`crates/js/src/shim/crypto_shim.js` — `new Promise(function (resolve, reject) { … _lumen_subtle_* … })`, `_to_bytes`; `crates/js/src/web_crypto_natives.rs`)

## Симптом

Каждый метод `crypto.subtle` выполняет операцию прямо в executor `new Promise(...)`, то есть синхронно, ещё до возврата промиса. Данные копируются через `Array.from(new Uint8Array(...))` в JS-массив чисел. Тест, который в цикле ставит сотни операций, держит главный поток всё время исполнения скрипта, и `load` не наступает:

- `derive_bits_keys/pbkdf2.https.any.html?1-1000` … `?7001-8000` (8 id) — ERROR `navigate: wait timeout: DocumentReady`. PBKDF2 с 100 000 итераций на каждый сабтест; варианты `.any.worker.html` того же теста доходят до конца (OK);
- `generateKey/failures_RSA-OAEP|RSA-PSS|RSASSA-PKCS1-v1_5.https.any.html` (3 id) — ERROR так же: проверки usages нет (BUG-1735), поэтому каждый «Bad usages» реально генерирует RSA-2048; `failures_RSA-OAEP.any.worker.html` — TIMEOUT (252/532);
- `digest/kangarootwelve`/`turboshake.tentative.https.any.html` (2 id) — ERROR: входы до 17⁶ ≈ 24 МБ (`ptn(Math.pow(17, 6))`) превращаются `Array.from` в массивы по 24 млн чисел ещё до проверки алгоритма.

## Как найдено

WPT-CRYPTO-SCOUT, кластер E: 15 id (14 ERROR + 1 TIMEOUT), потолок 14.5 балла (4.6 п.п. категории). Это единственные не-OK id категории.

## Что делать

1. Спецификация (§14.3, шаги «in parallel»): метод возвращает промис сразу, а операция идёт вне главного потока, результат приходит задачей в очередь crypto task source. Тяжёлые операции (PBKDF2, RSA-keygen, digest больших данных) выносить в пул потоков с резолвом промиса через очередь задач рантайма (так же, как сделаны другие асинхронные нативы `crates/js`). Минимум — откладывать работу хотя бы на задачу, чтобы не задерживать `load`.
2. Передавать байты в натив как `ArrayBuffer`/`Uint8Array` (копия через `slice`), без `Array.from` в массив чисел.
3. Проверки из BUG-1735 (usages до генерации) убирают RSA-keygen из `failures_RSA-*`. Это отдельный фикс, но оба вместе снимают ERROR.

## Как проверить

`run_corpus.py --prefixes WebCryptoAPI/derive_bits_keys/pbkdf2 WebCryptoAPI/generateKey/failures_RSA WebCryptoAPI/digest` — нет ERROR/TIMEOUT, harness OK у всех.
