# BUG-366 — `navigator.credentials` нарушает WebIDL: методы лежат на инстансе вместо прототипа, внутреннее `_get_original` торчит наружу перечислимым свойством, интерфейсные объекты вызываются без `new`

**Статус:** FIXED 2026-08-09
**Компонент:** js (`crates/js/src/credentials.rs` — тело шима `CredentialsContainer`; `crates/js/src/digital_credentials.rs` — утечка `_get_original`; установка — `crates/js/src/v8_runtime.rs:4596`/`4601`)
**Найден:** P2, WPT-VENDOR-fedcm (2026-07-28), проба `--dump-layout` вне WPT (все 81 id категории — SKIP по testdriver, прогон по построению не мог дать находок)

## Фикс (P3, 2026-08-09)

Реализованы все 5 пунктов «возможного фикса» из исходной находки:

1. `create`/`get`/`preventSilentAccess`/`store` перенесены с инстанса `container`
   на `CredentialsContainer.prototype` через `Object.defineProperty(...,
   {enumerable:false, writable:true, configurable:true})` (хелпер `defMethod`
   внутри `CREDENTIALS_SHIM`). `container` теперь `Object.create(CredentialsContainer.prototype)`
   без единого собственного свойства — `Object.keys(navigator.credentials)` пуст,
   `hasOwnProperty('create'/'get'/...)` — `false`.
2. `_get_original` убран из веб-видимого пространства целиком.
   `digital_credentials.rs` теперь патчит `Object.getPrototypeOf(navigator.credentials).get`
   (т.е. `CredentialsContainer.prototype.get`, доступный только после фикса п.1),
   а оригинальную функцию держит исключительно в замыкании обёртки —
   никакой персистентный флаг идемпотентности не нужен: `install_dom` создаёт
   новый V8-контекст на каждую навигацию, и оба установщика (`install_credentials_bindings_v8`,
   затем `install_digital_credentials_api_v8`) в нём вызываются ровно один раз
   (`v8_runtime.rs:4596`/`4601`), так что риска двойного оборачивания в проде нет.
3. `CredentialsContainer()` и `IdentityProvider()` теперь безусловно бросают
   `TypeError('Illegal constructor')`, как остальные интерфейсы в этом файле
   (`PublicKeyCredential`/`OTPCredential`/`IdentityCredential`). Внутреннее
   создание синглтона `container` по-прежнему идёт через
   `Object.create(CredentialsContainer.prototype)`, конструктор не вызывается —
   поведение не меняется.
4. `Symbol.toStringTag` добавлен на `CredentialsContainer.prototype`
   (`Object.prototype.toString.call(navigator.credentials)` теперь
   `[object CredentialsContainer]`).
5. `.constructor` восстановлен после каждой замены прототипа через
   `Object.create(...)`: `PublicKeyCredential`, `OTPCredential`,
   `IdentityCredential`, и заодно `AuthenticatorAttestationResponse`/
   `AuthenticatorAssertionResponse` — тот же паттерн-дефект был у всех пяти
   мест файла, не только у трёх, названных в исходной находке.

Новые тесты: `credentials::v8_fedcm::{credentials_methods_are_not_own_enumerable_properties,
credentials_container_constructor_throws_illegal_constructor,
identity_provider_constructor_throws_illegal_constructor,
credentials_container_has_tostringtag,
identity_credential_prototype_constructor_matches_own_class}` и
`digital_credentials::tests::{digital_wrap_does_not_leak_get_original_property,
digital_wrap_still_rejects_digital_get_on_real_container}` (последний — на
реальной паре установщиков `credentials.rs` → `digital_credentials.rs`, не на
изолированном фейке). `cargo test -p lumen-js --features v8-backend credentials` —
28/28 зелёных (24 в `lib.rs`-юниттестах + 4 в `tests/all.rs::cases::webauthn_credentials`).

## Симптом

Объект `navigator.credentials` — это **весь** вход в Credential Management: и
WebAuthn/passkeys (скоуп ⬜, реально реализованы), и WebOTP, и FedCM (скоуп 🚫,
осознанный стаб). Его форма отличалась от WebIDL по пяти пунктам сразу, и
один из них — утечка внутренней детали реализации в веб-видимое пространство.

Проба `--dump-layout` до фикса (все строки — фактический вывод):

```
credentials instanceof CredentialsContainer = true
get on prototype                            = false
get own prop of instance                    = true
own props of navigator.credentials          = create,get,preventSilentAccess,store,_get_original
CredentialsContainer.prototype props        = constructor
Object.keys(navigator.credentials)          = create,get,preventSilentAccess,store,_get_original
for-in visible                              = create,get,preventSilentAccess,store,_get_original
_get_original descriptor                    = enumerable=true writable=true configurable=true
typeof navigator.credentials._get_original  = function
toStringTag credentials                     = [object Object]
CredentialsContainer() without new          = no throw
IdentityProvider() without new              = no throw
IdentityCredential.prototype.constructor    = Credential
```

## Причина

1. **Методы были собственными свойствами инстанса, а не операциями прототипа.**
   `credentials.rs` создавал `var container = Object.create(CredentialsContainer.prototype)`
   (это как раз было правильно — `instanceof` работает), но дальше `create`/`get`/
   `preventSilentAccess`/`store` присваивались самому `container`. По WebIDL
   операции интерфейса живут на interface prototype object, поэтому
   `CredentialsContainer.prototype.get` должно быть функцией, а
   `navigator.credentials.hasOwnProperty('get')` — `false`. Было ровно наоборот,
   и `CredentialsContainer.prototype` содержал только `constructor`. Тот же
   класс дефекта, что пункт про обработчики событий в
   [BUG-363](BUG-363-FIXED.md) для `EventSource`.

   Практическое следствие, а не только буквоедство: перечислимость. Все четыре
   метода были видны в `Object.keys(navigator.credentials)` и в `for...in`,
   тогда как у настоящего `CredentialsContainer` `Object.keys()` пуст. Любой
   код, который перебирает свойства объекта (сериализация, полифилл-детекторы,
   фингерпринт-скрипты), видел другую картину.

2. **`_get_original` — внутренний guard, утёкший в веб.** `digital_credentials.rs`
   монки-патчил `navigator.credentials.get`, чтобы перехватить
   `options.digital`, и запоминал оригинал прямо на публичном объекте:
   `navigator.credentials._get_original = _orig;`. Свойство создавалось обычным
   присваиванием, то есть `enumerable=true writable=true configurable=true` —
   его было видно в `Object.keys` и `for...in`, его можно было прочитать и
   перезаписать из любого скрипта страницы. Это не спековое свойство, а деталь
   реализации:
   * оно однозначно опознавало браузер как Lumen — нежелательная поверхность
     фингерпринтинга для браузера, который позиционируется как приватный
     (`docs/plan/privacy.md`);
   * оно давало странице ссылку на *необёрнутый* `get`, минуя перехват
     `options.digital`;
   * оно же использовалось как флаг идемпотентности установки, поэтому
     страница, присвоившая `navigator.credentials._get_original` до установки
     шима, могла этот шим подавить.

3. **Интерфейсные объекты вызывались без `new`.** `CredentialsContainer()` и
   `IdentityProvider()` были обычными пустыми функциями, вызов без `new`
   проходил и возвращал `undefined`. По WebIDL interface object обязан
   бросать `TypeError`. Показательно, что рядом в том же файле это было
   сделано правильно: `PublicKeyCredential`, `OTPCredential` и
   `IdentityCredential` бросали `TypeError` — то есть пропущены были ровно те
   два интерфейса, у которых нет конструктора по спеке.

4. **`Symbol.toStringTag` отсутствовал.** `Object.prototype.toString.call(navigator.credentials)`
   давал `[object Object]` вместо `[object CredentialsContainer]`. Тот же
   пункт, что в [BUG-365](BUG-365-FIXED.md) для `EyeDropper`.

5. **`IdentityCredential.prototype.constructor === Credential`.**
   `IdentityCredential.prototype = Object.create(Credential.prototype)`
   заменяло прототип целиком и не восстанавливало `constructor`, поэтому он
   наследовался от `Credential.prototype`. Тем же были затронуты
   `PublicKeyCredential` и `OTPCredential`, а также, за пределами исходной
   находки, `AuthenticatorAttestationResponse` и `AuthenticatorAssertionResponse`
   — одна и та же строка-паттерн в пяти местах, все исправлены.

## Что было корректно и не тронуто фиксом

`navigator.credentials instanceof CredentialsContainer` === `true`;
`IdentityCredential.prototype instanceof Credential` === `true`;
`new IdentityCredential()` бросает `TypeError`; `navigator.credentials.get({identity:…})`
отклоняется `NotSupportedError` — ровно так, как обещает шапка модуля
(«FedCM API … is Phase 0: always rejects with `NotSupportedError`»), то есть
сам FedCM-стаб вёл себя как задокументировано и отдельным багом не был.

## Масштаб

Дефект лежал не в 🚫-скоупном FedCM, а в общем для всей категории объекте
`CredentialsContainer`, поэтому задевал и **в-скоупную** ветку WebAuthn/passkeys:
поверхность `navigator.credentials.create/get` для publicKey — та же самая.

Категория `fedcm` не может подтвердить фикс прогоном: у неё нет `idlharness`-теста,
а все 81 id — SKIP (`Executor does not support testdriver.js`), потому что каждый
тест FedCM по спеке требует пользовательского выбора аккаунта в браузерном
диалоге. Профильный WPT-тест для этого дефекта существует и уже вендорен —
`tests/wpt/credential-management/idlharness.https.window.js`
([ROADMAP.md](../ROADMAP.md) `WPT-VENDOR-credential-management`, 2026-07-26),
но он `.https.`-only и упирается в известный HTTPS-порт-гэп исполнителя,
поэтому автоматической WPT-проверки фикса по-прежнему нет — верификация ушла
в юнит-тесты движка (см. «Фикс» выше).

## Не найдено в этой категории (зафиксировано, чтобы не искали заново)

FedCM-стаб не реализует статики, на которых держится 89 из 103 тестовых файлов
категории: `IdentityCredential.disconnect` (20 обращений в тестах),
`IdentityProvider.resolve` (18), `IdentityProvider.close` (3) — все `undefined`;
целиком отсутствует FedCM Login Status API — `navigator.login`/`NavigatorLogin`
(12 обращений к `navigator.login.setStatus`), в `crates/` нет ни одного
упоминания. Это **не** заводится багом: отсутствие нереализованного API — честный
результат feature detection и ожидаемое состояние 🚫-скоупа, тот же класс, что
`delegated-ink`/`bluetooth`. Записано только чтобы будущая сессия не приняла
`undefined` за регрессию.
