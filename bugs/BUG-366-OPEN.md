# BUG-366 — `navigator.credentials` нарушает WebIDL: методы лежат на инстансе вместо прототипа, внутреннее `_get_original` торчит наружу перечислимым свойством, интерфейсные объекты вызываются без `new`

**Статус:** OPEN
**Компонент:** js (`crates/js/src/credentials.rs:358-418` — тело шима `CredentialsContainer`; `crates/js/src/digital_credentials.rs:42-44` — утечка `_get_original`; установка — `crates/js/src/v8_runtime.rs:3881`/`3886` для V8 и `crates/js/src/lib.rs:1053` для rquickjs)
**Найден:** P2, WPT-VENDOR-fedcm (2026-07-28), проба `--dump-layout` вне WPT (все 81 id категории — SKIP по testdriver, прогон по построению не мог дать находок)

## Симптом

Объект `navigator.credentials` — это **весь** вход в Credential Management: и
WebAuthn/passkeys (скоуп ⬜, реально реализованы), и WebOTP, и FedCM (скоуп 🚫,
осознанный стаб). Его форма отличается от WebIDL по четырём пунктам сразу, и
один из них — утечка внутренней детали реализации в веб-видимое пространство.

Проба `--dump-layout` (все строки — фактический вывод):

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

1. **Методы — собственные свойства инстанса, а не операции прототипа.**
   `credentials.rs:358` создаёт `var container = Object.create(CredentialsContainer.prototype)`
   (это как раз правильно — `instanceof` работает), но дальше `create`/`get`/
   `preventSilentAccess`/`store` присваиваются самому `container`
   (строки 360, 390, 417, 418). По WebIDL операции интерфейса живут на
   interface prototype object, поэтому `CredentialsContainer.prototype.get`
   должно быть функцией, а `navigator.credentials.hasOwnProperty('get')` —
   `false`. Сейчас ровно наоборот, и `CredentialsContainer.prototype` содержит
   только `constructor`. Тот же класс дефекта, что пункт про обработчики
   событий в [BUG-363](BUG-363-FIXED.md) для `EventSource`.

   Практическое следствие, а не только буквоедство: перечислимость. Все четыре
   метода видны в `Object.keys(navigator.credentials)` и в `for...in`, тогда
   как у настоящего `CredentialsContainer` `Object.keys()` пуст. Любой код,
   который перебирает свойства объекта (сериализация, полифилл-детекторы,
   фингерпринт-скрипты), увидит другую картину.

2. **`_get_original` — внутренний guard, утёкший в веб.** `digital_credentials.rs:44`
   монки-патчит `navigator.credentials.get`, чтобы перехватить
   `options.digital`, и запоминает оригинал прямо на публичном объекте:
   `navigator.credentials._get_original = _orig;`. Свойство создаётся обычным
   присваиванием, то есть `enumerable=true writable=true configurable=true` —
   его видно в `Object.keys` и `for...in`, его можно прочитать и перезаписать
   из любого скрипта страницы. Это не спековое свойство, а деталь реализации:
   * оно однозначно опознаёт браузер как Lumen — нежелательная поверхность
     фингерпринтинга для браузера, который позиционируется как приватный
     (`docs/plan/privacy.md`);
   * оно даёт странице ссылку на *необёрнутый* `get`, минуя перехват
     `options.digital`; сейчас это безобидно (обе ветки всё равно отклоняют
     запрос), но перестанет быть безобидным, как только у перехвата появится
     смысл;
   * оно же используется как флаг идемпотентности установки (`typeof
     navigator.credentials._get_original === 'undefined'`, строка 42), поэтому
     страница, присвоившая `navigator.credentials._get_original` до установки
     шима, может этот шим подавить.

   Правильное место для такого состояния — замыкание модуля или неперечислимое
   свойство с символьным ключом, а не публичное имя на веб-объекте.

3. **Интерфейсные объекты вызываются без `new`.** `CredentialsContainer()` и
   `IdentityProvider()` (`credentials.rs:306` и `302`) — обычные пустые
   функции, вызов без `new` проходит и возвращает `undefined`. По WebIDL
   interface object обязан бросать `TypeError`. Показательно, что рядом в том
   же файле это сделано правильно: `PublicKeyCredential`, `OTPCredential` и
   `IdentityCredential` (строки 294, 296, 299) бросают `TypeError` — то есть
   пропущены ровно те два интерфейса, у которых нет конструктора по спеке.

4. **`Symbol.toStringTag` отсутствует.** `Object.prototype.toString.call(navigator.credentials)`
   даёт `[object Object]` вместо `[object CredentialsContainer]`. Тот же пункт,
   что в [BUG-365](BUG-365-OPEN.md) для `EyeDropper`.

5. **`IdentityCredential.prototype.constructor === Credential`.** Строка 300
   (`IdentityCredential.prototype = Object.create(Credential.prototype)`)
   заменяет прототип целиком и не восстанавливает `constructor`, поэтому он
   наследуется от `Credential.prototype`. Тем же затронуты `PublicKeyCredential`
   и `OTPCredential` (строки 295, 297) — одна и та же строка-паттерн в трёх
   местах.

## Что при этом корректно и ломать при фиксе не надо

Проверено той же пробой: `navigator.credentials instanceof CredentialsContainer`
=== `true`; `IdentityCredential.prototype instanceof Credential` === `true`;
`new IdentityCredential()` бросает `TypeError`; `navigator.credentials.get({identity:…})`
отклоняется `NotSupportedError` — ровно так, как обещает шапка модуля
(«FedCM API … is Phase 0: always rejects with `NotSupportedError`»), то есть
**сам FedCM-стаб ведёт себя как задокументировано** и отдельным багом не
является. Это существенное отличие от `eyedropper`/[BUG-365](BUG-365-OPEN.md),
где стаб был сломан относительно собственной документации.

## Масштаб

Дефект лежит не в 🚫-скоупном FedCM, а в общем для всей категории объекте
`CredentialsContainer`, поэтому задевает и **в-скоупную** ветку WebAuthn/passkeys:
поверхность `navigator.credentials.create/get` для publicKey — та же самая.

Категория `fedcm` подтвердить фикс не сможет: у неё нет `idlharness`-теста, а
все 81 id — SKIP (`Executor does not support testdriver.js`), потому что каждый
тест FedCM по спеке требует пользовательского выбора аккаунта в браузерном
диалоге. Верификация — пробой `--dump-layout` (три страницы пробы лежат в
`.tmp/fedcm-probe*.html` этой сессии, не коммичены). Профильный WPT-тест для
этого дефекта существует и уже вендорен —
`tests/wpt/credential-management/idlharness.https.window.js`
([ROADMAP.md](../ROADMAP.md) `WPT-VENDOR-credential-management`, 2026-07-26),
но он `.https.`-only и упирается в известный HTTPS-порт-гэп исполнителя,
поэтому автоматической проверки фикса сегодня нет ни в одной категории.

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

## Возможный фикс (не реализован в этой сессии)

1. Перенести `create`/`get`/`preventSilentAccess`/`store` на
   `CredentialsContainer.prototype` через `Object.defineProperty` с
   `enumerable:false, writable:true, configurable:true`; `container` тогда
   станет `Object.create(CredentialsContainer.prototype)` без собственных
   свойств вовсе.
2. Убрать `_get_original` из веб-видимого пространства: держать оригинальный
   `get` в замыкании `digital_credentials.rs`, а флаг идемпотентности — в
   неперечислимом свойстве с символьным ключом (или в Rust-состоянии установки).
   После п.1 перехват `options.digital` естественно переезжает на патч
   `CredentialsContainer.prototype.get`.
3. `CredentialsContainer` и `IdentityProvider` — бросать `TypeError` при вызове
   (и с `new` тоже: у обоих нет конструктора по спеке), как уже сделано у
   `PublicKeyCredential`/`OTPCredential`/`IdentityCredential`.
4. Проставить `Symbol.toStringTag` и восстановить `constructor` во всех трёх
   местах замены прототипа (строки 295, 297, 300).
5. Верификацию вести на наблюдаемой форме (`Object.keys(navigator.credentials)`
   должен быть пуст, `CredentialsContainer.prototype.hasOwnProperty('get')` ===
   `true`), а не на `typeof navigator.credentials.get === 'function'` — текущий
   набор `credentials::tests` проверяет именно последнее и останется зелёным
   при любом из перечисленных дефектов.

Не чинится в этой сессии — P2-wpt вендорит и обследует, фиксы кода — дорожка P3
(`CLAUDE.md`, назначения разработчиков).
