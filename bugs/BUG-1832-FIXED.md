# BUG-1832 — дефолтная идентичность `chrome_lumen`: токен `Lumen/x.y` в UA и Client Hints по умолчанию, per-site `chrome_unbranded` для fiverr.com

**Статус:** FIXED 2026-11-19
**Заведён:** 2026-11-19 (workstream C плана `docs/plans/fingerprint-unification-plan.md`, ветка `fingerprint-identity-switch`)
**Компонент:** core (`crates/core/src/identity.rs`), network (`http/headers.rs`, `http/client_hints.rs`), shell (`config.rs`), update-manifest (`assets/compat_pack.json`)

## Контекст

ADR-034 §1 санкционировал переход дефолта на «Chrome-грамматику + хвостовой токен
`Lumen/x.y`» (модель Edge/Ladybird) после A/B-матрицы 2026-10-15 (533 сайта,
100 must-work): токен и бренд `Lumen` в `Sec-CH-UA` блокируют ровно 1 сайт =
0,19 % (fiverr-com, PerimeterX), ни одного must-work; порог ≤ 0,5 % выполнен —
см. [матрицу](../docs/perf/runs/2026-10-15-ab-matrix.md) и
[ADR-034](../docs/decisions/ADR-034-site-compatibility-strategy.md) §1
(амендмент 2026-11-19). До этого кода дефолтом оставался `ChromeUnbranded`.

## Что изменено

- `IdentityVariant::default()` → `ChromeLumen`; `LUMEN_TOKEN` стартует с `true`
  (`crates/core/src/identity.rs`). Откат — мгновенный: `identity =
  "chrome_unbranded"` в `fingerprint.toml` или `LUMEN_IDENTITY=chrome_unbranded`.
- Тесты перевернуты под новый дефолт: `variant_parses_both_spellings_and_rejects_the_rest`,
  `test_h2_fingerprint_headers_are_lowercase_and_drop_connection_headers`
  (теперь ждёт `CHROME_LUMEN_USER_AGENT`), `test_chrome_sends_sec_ch_ua_on_h1_and_h2`
  (Chrome/Strict несут токен и бренд; Edge выделен и по-прежнему без токена),
  `test_client_hints_headers_chrome` (бренд `Lumen` в `Sec-CH-UA`),
  `identity_key_parses_and_env_wins` (дефолт и fallback → `ChromeLumen`).
- Первая реальная запись compat-пакета (revision 1):
  `fiverr-com-identity-unbranded` — `matches: ["fiverr.com"]`,
  `action: identity → chrome_unbranded`, `review_by: 2027-02-19`.
  `embedded_base_copy_is_valid` проверяет `fiverr.com`/`www.fiverr.com` →
  `ChromeUnbranded`, прочие хосты → `None`.
- Доки синхронно: `CAPABILITIES.md`, `docs/automation.md` (`LUMEN_IDENTITY`),
  амендмент в Status ADR-034.

## Проверки

- `cargo test -p lumen-core`, `cargo test -p lumen-update-manifest`,
  `cargo test -p lumen-network --lib http`, `cargo test -p lumen-shell --bin lumen config::`
- Живые проверки и A/B `captcha_stand.py` — артефакты в `docs/perf/runs/` и
  `docs/perf/journal.md` того же коммита (см. план §3.2).
