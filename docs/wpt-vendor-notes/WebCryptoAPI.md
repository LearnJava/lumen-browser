# WPT vendor notes — `WebCryptoAPI`

## Vendoring (`tests/wpt/VENDOR.md`)

Fourth full test category, added 2026-07-22 by the WPT-VENDOR backlog (`ROADMAP.md` `WPT-VENDOR-WebCryptoAPI`, `docs/wpt-status.md`). Same pinned commit, `git sparse-checkout` at the same commit hash (reused from the IndexedDB sparse checkout), 185 files. References only the already-vendored `/resources/testharness.js`+`testharnessreport.js` — no out-of-category helpers. 122/125 test files are `.https.`-only (`crypto.subtle` requires a secure context), which the minimal `wptrunner` executor cannot serve (no HTTPS port configured) — a documented survey gap, not a Lumen engine bug; see `docs/wpt-status.md` for the isolated run of the 2 non-`.https.` files.

## Прогон и находки (`docs/wpt-status.md`)

Вендорена целиком 2026-07-22 (коммит `629dbeb5`, `tests/wpt/WebCryptoAPI/`, 185 файлов; ссылается только на уже вендоренные `/resources/testharness.js`+`testharnessreport.js`, внекатегорийных хелперов нет). 122/125 тестовых файлов — `.https.`-only (спека требует secure context для `crypto.subtle`); прогон `run_report.py --all --root WebCryptoAPI --recursive` подтвердил на 73 добежавших id, что все они падают одинаково (`invalid url: invalid port: "None"` — минимальный исполнитель не поднимает HTTPS-порт, тот же класс ограничения, что и `FileAPI`/`BlobURL`, здесь просто доминирует почти всю категорию); полный прогон дважды не добрался до конца (зависание раннера на многочасовой отметке — не сбой браузера, `WebSocket connection closed` со стороны wptrunner), поэтому по `.https.`-подмножеству детальной разбивки по сабтестам нет. 2 не-`.https.` файла (`getRandomValues.any.js`, `historical.any.js`) прогнаны изолированно: 2/2 harness OK, 28/42 сабтестов passed. Замеченные кластеры провалов (не заведены как BUG-NNN — первый проход, см. методологию выше): `crypto.getRandomValues()` не бросает `TypeMismatchError` на `Float16/32/64Array`/`DataView` (спека требует отказа для не-целочисленных TypedArray); брошенный при превышении 65536 байт `QuotaExceededError` не соответствует спеке по форме (нет ожидаемых `requested`/`quota`); `crypto.subtle`/`SubtleCrypto`/`CryptoKey` доступны из небезопасного (http, не secure context) контекста — secure-context gate для Web Crypto отсутствует вовсе

## Baseline expectations (WPT-RUN-7-S4, 2026-10-18)

Есть `tests/wpt/metadata/WebCryptoAPI/` (125 `.ini`): 313 инстансов, ~172 harness OK, ~2,0 тыс. из ~18 тыс. подтестов PASS. Причина провалов — пробел функциональности: SubtleCrypto не знает Ed25519/Ed448, X25519/X448, ML-KEM/ML-DSA, AES-KW/OCB, ChaCha20-Poly1305, KMAC, SHA-3/TurboSHAKE/KangarooTwelve, Argon2 (`NotSupportedError`), а в dedicated worker нет `crypto`/`SubtleCrypto`. `generateKey/failures_*` и `successes_RSA-*` флапают между harness `ERROR`/`TIMEOUT` и `OK` — в `.ini` объединённые значения плюс `TIMEOUT`. Подробности и метод — `docs/tasks/p2-test-track.md#test-3-срез-72-2026-10-18`. Прогонять с `--processes 4`: на 7 процессах wptrunner теряет ~90 id.

## Разведка WPT-CRYPTO-SCOUT (2026-10-27)

Статус: закрыта 2026-10-27. Баги BUG-1734…1739 (очередь P3), задачи WEBCRYPTO-CURVES, WEBCRYPTO-SYM, WEBCRYPTO-PQ (очередь P1, после BUG-1736). Код не менялся.

**Прогон** (`run_corpus.py --prefixes WebCryptoAPI --processes 6`, `dev-release` от `main` `0e7f0a78a`, уже с фиксом BUG-1688): 313 id, **13.53 %** (было 4.67 %), балл 42.35, harness OK 298, сабтесты 12 804 / 62 750 (с воркерами сабтестов стало вчетверо больше); статусы OK 298, ERROR 14, TIMEOUT 1; 367 с.

**Главное.** Шим `crates/js/src/shim/crypto_shim.js` — литерал-объект без нормализации алгоритма: `CryptoKey` без `Symbol.toStringTag`/`algorithm.length` ломает первую же проверку всех «успешных» тестов (A); `NotSupportedError` вместо `SyntaxError`/`TypeError`/`InvalidAccessError` и «успех» там, где нужна ошибка (B); половина потерь — алгоритмы, которых нет (C).

### Таблица кластеров

Балл — потолок (Σ потерянных долей сабтестов, каждый упавший сабтест отнесён к одной причине — первой совпавшей); реальный прирост ниже: после фикса A в тех же сабтестах всплывают B/C. Сумма = 270.6 потерянных балла.

| Кл. | Группа | id | Сабтестов | Статус | Причина (представитель) | Балл (п.п.) |
|---|---|---|---|---|---|---|
| A | форма `CryptoKey` | 66 | 4 684 | FAIL | движок: нет `Symbol.toStringTag`, `algorithm.length` у AES/HMAC, имя `RSASSA-PKCS1-V1_5`, usages пары RSA, `extractable` при импорте — [BUG-1734](../../bugs/BUG-1734-OPEN.md). `successes_RSA-OAEP` (32 id) | 58.7 (18.8) |
| A2 | WebIDL `Crypto`/`SubtleCrypto`/`CryptoKey`, `getRandomValues` | 8+4 | 820+24 | FAIL | движок: нет глобала `SubtleCrypto`, методы — own-свойства; Float16/DataView, `QuotaExceededError` — [BUG-1735](../../bugs/BUG-1735-OPEN.md). `idlharness` | 7.6 (2.4) |
| B | normalize algorithm, usages, порядок ошибок | 107 | 23 274 | FAIL | движок: нет §18.4; `generateKey/failures_*` «Bad usages»/«Empty algorithm»/«Bad algorithm», `*_importKey_failures_*`, hkdf/pbkdf2 «wrong (ECDH) key»/«bad hash name»/«missing deriveKey usage», `normalize-algorithm-name` — [BUG-1736](../../bugs/BUG-1736-OPEN.md) | 39.6 (12.6) |
| C1 | кривые: P-384/P-521, Ed25519/Ed448, X25519/X448, `getPublicKey` | ≈70 | — | FAIL (NotSupportedError) | фича: `WEBCRYPTO-CURVES` | ≈41 (13) |
| C2 | симметричное и хеши: AES-192, AES-KW + `wrapKey`/`unwrapKey`, AES-OCB, ChaCha20-Poly1305, KMAC, SHA-3/cSHAKE/TurboSHAKE/K12, Argon2 | ≈60 | — | FAIL | фича: `WEBCRYPTO-SYM` | ≈38 (12) |
| C3 | постквантовые: ML-KEM (+`encapsulate*`), ML-DSA, `raw-*` форматы, `SubtleCrypto.supports` | ≈20 | — | FAIL | фича: `WEBCRYPTO-PQ` (tentative) | ≈18 (5.7) |
| D | `structuredClone(CryptoKey)` | 38 | 126 | FAIL | движок: клон — простой объект, «exportKey: argument is not a CryptoKey» — [BUG-1737](../../bugs/BUG-1737-OPEN.md); 18 id поддержанных алгоритмов | 38.0 (12.1), из них ≈17 без C |
| E | синхронные операции блокируют `load` | 15 | — | ERROR 14 / TIMEOUT 1 | движок: работа в executor промиса — pbkdf2 window ×8, `failures_RSA-*` window ×3 + worker TIMEOUT, kangarootwelve/turboshake window — [BUG-1738](../../bugs/BUG-1738-OPEN.md) | 14.5 (4.6) |
| G | неверный результат | 35 | 1 967 | FAIL | движок: `tagLength` AES-GCM, `saltLength` RSA-PSS, AES-CTR, `length` deriveBits — [BUG-1739](../../bugs/BUG-1739-OPEN.md) | 4.7 (1.5) |
| F | «transferred buffer during call» | 25 | 488 | FAIL | движок: данные копируются после `JSON.stringify(algorithm)` — BUG-1739 | 3.0 (1.0) |
| H | прочее | 8 | 47 | FAIL | `historical` — **раннер**: тест ждёт небезопасный контекст, а `http://localhost` безопасен по Secure Contexts (на wpt.fyi — `web-platform.test`), не чинится; `algorithm-discards-context.window` — `iframe.contentDocument.body` null сразу после `appendChild` ([BUG-480](../../bugs/BUG-480-OPEN.md)) | 4.6 (1.5) |

Раннер/бюджет: собственных проблем раннера нет — ERROR/TIMEOUT кластера E даёт движок (на воркерах те же тесты OK). Скрипты разбора: `.tmp/crypto_cl.py` (классификатор), `.tmp/crypto_files.py` (потери по файлам).
