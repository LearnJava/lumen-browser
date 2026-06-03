# Задача: ADR-011 — crypto зависимости (hmac + aes-gcm)

**Developer:** P1  
**Ветка:** `p1-adr-crypto`  
**Размер:** XS (только документация, Rust-код не трогаем)  
**Крейты:** нет (только `docs/`)

---

## Контекст

Коммит `p1-subtle-crypto` (ветка слита 2026-06-03) добавил `hmac v0.12` и `aes-gcm v0.10`
в `crates/js/Cargo.toml` без формального ADR и без строки «Why this dependency»
в теле коммита — нарушение CLAUDE.md §«No new dep without justification».
P5 зафиксировал это в STATUS-P1 "Next". Нужно оформить ADR-011.

---

## Пред-запуск

- [ ] Прочесть `docs/decisions/TEMPLATE.md` (формат ADR)
- [ ] Прочесть `docs/decisions/README.md` (последний номер = ADR-010)
- [ ] Убедиться, что ветка `main` чиста: `git status`

---

## Шаги

### 1. Создать ветку

```bash
export PATH="/c/Users/konstantin/.cargo/bin:$PATH"
git worktree add .claude/worktrees/adr-crypto -b p1-adr-crypto
cd .claude/worktrees/adr-crypto
```

### 2. Создать файл ADR-011

Создать файл `docs/decisions/ADR-011-crypto-deps.md` со следующим содержимым:

```markdown
# ADR-011: Provisional crypto deps — hmac + aes-gcm (SubtleCrypto API)

## Status

Accepted

## Date

2026-06-03

## Context

The W3C WebCryptography API (`SubtleCrypto`) requires HMAC (SHA-256/384/512
signing/verification) and AES-GCM (128/256-bit encrypt/decrypt with AAD) as
mandatory algorithms (WebCryptography §14). These algorithms are used in
`crates/js/src/subtle_crypto.rs` to implement `crypto.subtle.sign`,
`crypto.subtle.verify`, `crypto.subtle.encrypt`, `crypto.subtle.decrypt`.

Phase 0 needs working SubtleCrypto so that pages relying on JWT tokens, FIDO2
authentication flows, and encrypted storage do not throw `NotSupportedError`.

`p256` (ECDSA P-256) was already a permanent dependency in `lumen-network`
(used for TLS, WebAuthn). `sha2` is a transitive dep of `p256`. Only
`hmac` and `aes-gcm` are new additions.

## Decision

Add `hmac = "0.12"` and `aes-gcm = "0.10"` to `crates/js/Cargo.toml`
as **Provisional** dependencies, trait-anchored to the `SubtleCrypto` JS API
surface (`crates/js/src/subtle_crypto.rs`).

Category: **Provisional** (ADR-002 §3.2).

Trait-anchor: `SubtleCrypto` — `window.crypto.subtle` object in JS runtime.

Graduation criterion: when Phase 1 ships the full Web Crypto API to end-users
OR when the project switches to an own pure-Rust HMAC/AES-GCM implementation.
Until then, these crates remain provisional and must not be exposed at the
`lumen-core` or `lumen-network` level.

## Alternatives considered

| Alternative | Why rejected |
|---|---|
| `ring` crate | Not pure Rust; requires C toolchain; conflicts with existing `p256`/`sha2` RustCrypto chain |
| `openssl` crate | C FFI dependency; too heavy; breaks cross-compilation story |
| Pure-Rust own impl | HMAC is trivial but AES-GCM (GHASH, CTR) is non-trivial; crypto must be audited; deferred to Phase 2 |
| Feature-gate behind `js-crypto` | Would complicate build matrix for minimal value in Phase 0 |

## Consequences

- **Positive:** SubtleCrypto tests pass; pages using `crypto.subtle` no longer throw.
- **Positive:** Uses the RustCrypto ecosystem consistently (`hmac` + `aes-gcm` share
  the same `digest`/`cipher` traits as `sha2`/`p256` already in use).
- **Negative:** Two new transitive deps added to `crates/js` dep graph.
- **Future:** Graduate to permanent when Phase 1 ships, or replace with
  `lumen-crypto` crate once own HMAC/AES-GCM implementation is audited.
```

### 3. Обновить индекс ADR

Файл `docs/decisions/README.md`.

Найти последнюю строку таблицы Index (сейчас это ADR-010) и добавить **после неё**:

```
| [ADR-011](ADR-011-crypto-deps.md) | Provisional crypto deps — hmac + aes-gcm (SubtleCrypto API) | Accepted | 2026-06-03 |
```

### 4. Обновить STATUS-P1.md

В файле `STATUS-P1.md` в секции `## Next`:

Найти строку, начинающуюся с `- **ADR для crypto-зависимостей**`, и **удалить** весь
этот многострочный пункт (он занимает ~8 строк).

Затем в секции `## Recent merges` добавить вверху:

```
- **p1-adr-crypto** ✅ 2026-06-03 — ADR-011: оформлены deps hmac v0.12 + aes-gcm v0.10 (SubtleCrypto, Provisional, CLAUDE.md §No new dep). Обновлён индекс docs/decisions/README.md.
```

### 5. Проверить

Код не меняется → clippy не нужен. Проверить только что файлы созданы:

```bash
ls docs/decisions/ADR-011-crypto-deps.md
grep "ADR-011" docs/decisions/README.md
grep "ADR-011" docs/decisions/README.md | wc -c
```

### 6. Закоммитить и влить

```bash
git add docs/decisions/ADR-011-crypto-deps.md docs/decisions/README.md STATUS-P1.md
git commit -m "P1: ADR-011 — crypto deps hmac + aes-gcm (SubtleCrypto, Provisional)

Оформляет задолженность из p1-subtle-crypto: hmac v0.12 + aes-gcm v0.10
добавлены без строки «Why this dependency». ADR-011 содержит контекст,
категорию Provisional, trait-anchor SubtleCrypto, graduation criterion.

Co-Authored-By: Claude Haiku 4.5 <noreply@anthropic.com>"

cd ../..
git merge --no-ff p1-adr-crypto -m "Merge p1-adr-crypto: ADR-011 crypto deps (RustCrypto hmac+aes-gcm)"
git branch -d p1-adr-crypto
git push origin main
git worktree remove .claude/worktrees/adr-crypto
```

---

## Критерии готовности

- [ ] Файл `docs/decisions/ADR-011-crypto-deps.md` существует
- [ ] `docs/decisions/README.md` содержит строку ADR-011
- [ ] `STATUS-P1.md` "Next" больше не содержит пункт про ADR crypto
- [ ] `STATUS-P1.md` "Recent" содержит `p1-adr-crypto`
