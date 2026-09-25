# BUG-709: WebAuthn `create()`/`get()` never validate `rp.id`/`rpId` against the calling document's origin — no origin-binding enforcement at all

**Статус:** FIXED 2026-09-17 (P3)
**Компонент:** js (`crates/js/src/credentials.rs` — `CREDENTIALS_SHIM`'s `create`/`get` methods) and network (`crates/network/src/webauthn.rs` — `VirtualAuthenticator::create`/`get`)
**Найден:** WPT-VENDOR-webauthn (`ROADMAP.md`), code read + a temporary local `#[test]` probe (removed before commit; `git diff` on the test file is empty)

## Симптом

WebAuthn's core anti-phishing guarantee — that a relying-party id (`rp.id` /
`rpId`) must equal the calling document's origin or be a registrable-domain
suffix of it (W3C WebAuthn L2 §5.1.3, "creating a credential" / "the client
must confirm that `rpId` matches the effective domain") — is not enforced
anywhere in the create/get path:

1. **JS shim** (`CREDENTIALS_SHIM` in `credentials.rs`): `rp.id` is read
   straight from caller-supplied `options.publicKey.rp.id` (falling back to
   `currentHost()` only if omitted) and packed into the native request with
   no comparison against `location.hostname`/`location.origin` at all.
   Same for `get()`'s `pk.rpId`.
2. **Native bindings** (`credentials.rs::create`/`get`, the
   `_lumen_webauthn_create`/`_lumen_webauthn_get` handlers): unpack the
   packed fields and forward `rp_id`/`origin` verbatim to the installed
   `CredentialProvider` — no validation step exists between unpacking and
   dispatch.
3. **`VirtualAuthenticator::create`/`get`** (`crates/network/src/webauthn.rs`):
   stores/looks up credentials keyed by whatever `rp_id` string it was
   handed; never compares it against `req.origin`.

Confirmed with a temporary test added to
`crates/js/tests/cases/webauthn_credentials.rs` (removed before commit): a
V8 runtime installed at document origin `https://example.com/login` (via
`install_dom`) calls

```js
navigator.credentials.create({ publicKey: {
  rp: { id: 'attacker-controlled-unrelated.example', name: 'Not Us' },
  user: { id: new Uint8Array([1]).buffer, name: 'x', displayName: 'x' },
  challenge: new Uint8Array([1]).buffer,
  pubKeyCredParams: [{ type: 'public-key', alg: -7 }]
}})
```

and the promise **resolves** (`resolved:AQID`) instead of rejecting with
`SecurityError`; the canned provider records
`req.rp_id == "attacker-controlled-unrelated.example"` and
`req.origin == "https://example.com"` — a completely unrelated domain, sent
through unchanged. A real relying-party library on the other end has no way
to detect this from the response alone (`clientDataJSON.origin` is separately
built from the same unchecked `req.origin`, so at least that field is
internally consistent — but nothing stopped `rp_id` from diverging from it in
the first place).

## Почему это баг, а не просто Phase-0-заглушка

Unlike most other WebAuthn-adjacent gaps (WebOTP, FedCM), the code here is
not a stub: `VirtualAuthenticator` is a fully real, unit-tested ES256
authenticator (`crates/network/src/webauthn.rs` module doc: "a real ES256
… key store", RFC 6979 deterministic signatures, valid COSE keys). The
existing test suite
(`crates/js/tests/cases/webauthn_credentials.rs::create_returns_public_key_credential`)
only ever exercises the same-origin case (`rp.id: 'example.com'` on a page at
`https://example.com/login`), so the gap was never exercised by CI. Origin
binding is *the* security property WebAuthn exists to provide (a page can't
mint or assert a credential scoped to a domain it doesn't control) — its
total absence here means that, the moment a `CredentialProvider` is wired
into the shell (currently `set_credential_provider` is never called anywhere
under `crates/shell/`, confirmed via `grep -rln set_credential_provider
crates/shell/` — zero hits, which is why `WPT-VENDOR-webauthn`'s scope is
correctly marked 🚫 today and every real `.https.` WPT test in the category
TIMEOUTs on the unrelated TLS gap [BUG-657](BUG-657-FIXED.md) rather than
reaching this code), any page could mint/assert credentials against any
other origin's `rp.id` with zero client-side gate.

## Живая проба

`securecontext.http.html` (the one non-`.https.` WPT test in the category
that actually executed) independently reconfirmed the already-open
[BUG-399](BUG-399-FIXED.md) (`window.isSecureContext` hardcoded `true`):
`FAIL no navigator.credentials.create in non-secure context - assert_false:
expected false got true`. Not a new bug — folded into the existing BUG-399
reconfirmation list, not counted separately here.

## Дальше

Add an origin-binding check as the very first step of both `create()`/`get()`
in `CREDENTIALS_SHIM` (cheapest place — rejects before any native call, no
new plumbing needed): compute the calling document's effective domain from
`location.hostname`, and reject with `mkErr('SecurityError', ...)` unless
`rp.id`/`pk.rpId` equals it or is a proper registrable-domain suffix of it
(the same suffix-match logic `document.domain` relaxation already needs
elsewhere in the codebase, if it exists, should be reused rather than
reimplemented). Belt-and-suspenders: also add the check natively in
`credentials.rs::create`/`get` (`WebAuthnCreateRequest`/`WebAuthnGetRequest`
would need an `effective_domain` field carrying the browser's own — not
JS-supplied — notion of current origin) so a future non-shim caller of the
native bindings can't bypass a JS-only gate.

## Исправлено (2026-09-17, P3)

Добавлена origin-binding проверка `rp.id`/`pk.rpId` в двух независимых
местах, как и намечено в §Дальше выше:

1. **JS-шим** (`CREDENTIALS_SHIM` в `credentials.rs`): новая функция
   `rpIdMatchesHost(rpId, host)` — та же suffix-логика, что уже использует
   сеттер `document.domain` (`web_api_shim_mid.js`, "relaxing the
   same-origin restriction"): `rpId` обязан совпадать с эффективным доменом
   документа или быть его registrable-domain суффиксом (граница по точке,
   не просто строковый суффикс — `evil-example.com` не матчит
   `example.com`). Проверка стоит ДО проверки `typeof _lumen_webauthn_create
   !== 'function'`, так как это валидация входных данных, а не наличия
   авторизатора — так что она отклоняет запрос `SecurityError` даже без
   установленного `CredentialProvider`.
2. **Native** (`credentials.rs::create`/`get`): `rp_id_matches_origin(rp_id,
   origin)` — вместо отдельного поля `effective_domain`, для которого
   потребовался бы новый плюмбинг через `WebAuthnCreateRequest`/
   `WebAuthnGetRequest`, переиспользован уже существующий `req.origin`:
   это `location.origin` шима, ReadOnly для страницы (в отличие от `rp.id`,
   который приходит из caller-supplied `options`), так что доверять ему
   безопасно. `origin_host()` вытаскивает хост из `scheme://host[:port]`
   (включая IPv6-literal `[::1]:port`), затем та же label-boundary suffix
   проверка. Это страховка независимая от JS-гейта — future non-shim
   caller нативных биндингов её не обойдёт.

Новые тесты (`crates/js/src/credentials.rs`):
- `origin_host_strips_scheme_port_and_path`, `rp_id_matches_origin_accepts_same_origin_and_registrable_suffix`,
  `rp_id_matches_origin_rejects_unrelated_and_partial_label_match` — юнит-тесты
  на чистые функции, включая label-boundary edge case.
- `create_and_get_reject_rp_id_not_matching_origin` — сквозной тест через
  `create()`/`get()` с провайдером-паникером (`unreachable!` в `create`/`get`
  трейта) — доказывает, что провайдер не вызывается при мисматче.
- `create_rejects_rp_id_unrelated_to_calling_origin`,
  `get_rejects_rp_id_unrelated_to_calling_origin`,
  `create_accepts_rp_id_that_is_registrable_suffix_of_origin` (в
  `v8_fedcm`-модуле) — гоняют сам JS-шим через `V8JsRuntime`; вскрыт нюанс
  V8's `MicrotasksPolicy::Auto` — промис, синхронно отклонённый внутри
  своего executor'а, доставляет реакцию `.then`/`.catch` только как
  микротаск, который дренируется по возврату из ТЕКУЩЕГО top-level script —
  поэтому синхронное чтение результата в том же `eval()` всегда читает
  устаревшее значение; тест разбит на два вызова `eval()` (settle → read).

`cargo test -p lumen-js --lib credentials:: --features v8-backend` — 30/30.
`cargo clippy --workspace --all-targets -- -D warnings` — чисто.
`scripts/scoped-test.sh` — единственный красный тест
(`cases::snapshot_cpu::cpu_snapshots_match_references`, те же 7 файлов)
совпадает byte-for-byte с уже задокументированным чужим дрейфом
[BUG-1008](BUG-1008-OPEN.md), не регрессия. Только JS/native-биндинги,
пиксели не затронуты.
