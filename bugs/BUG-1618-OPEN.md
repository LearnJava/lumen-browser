# BUG-1618 — Панель сертификата всегда пишет «нет CT» и «нет OCSP»

**Статус:** OPEN
**Заведён:** 2026-10-12 (координационная сессия: сверка задач по `docs/research/` с кодом на `main`)
**Область:** network/shell (`tls/ct_logs.rs:33`, `tls/ct.rs:303`, `network/src/lib.rs:1492`)

## Симптом и место в коде

Две строки панели сертификата (Ctrl+Shift+C, cert_panel.rs:176-199) неверны на любом сайте. CT: KNOWN_LOG_IDS = &[] (ct_logs.rs:33), а evaluate_ct_against засчитывает только SCT известных журналов (ct.rs:303-317). Поэтому лист с 2-3 встроенными SCT получает Insufficient(0), и панель пишет «No CT evidence found» (fingerprint.rs:225). Отзыв: cert_info_from_completed_handshake жёстко передаёт OcspVerdict::Unknown (lib.rs:1492), и панель пишет «No revocation info (no stapled OCSP)» (fingerprint.rs:216), даже когда сервер прислал stapled Good. В комментарии lib.rs:1478-1484 это названо «safe under-statement», но текст утверждает ложное. Документы тоже преувеличивают. CAPABILITIES.md:232 обещает «real revocation/CT rows», а там же фраза «no dedicated chrome-DOM screen yet» устарела: #securityBox есть (chrome.html:1049). Note строки P3-tlshard в ROADMAP утверждает «CT log enforcement … завершена», хотя CT нигде не применяется. Владелец указан P6, потому что P3 нет в списке ролей.

## Где смотреть

- crates/network/src/tls/ct_logs.rs:33
- crates/network/src/tls/ct.rs:303-317
- crates/network/src/tls/fingerprint.rs:216
- crates/network/src/tls/fingerprint.rs:225
- crates/network/src/lib.rs:1488-1494
- crates/shell/src/panels/cert_panel.rs:176-199
- CAPABILITIES.md:232
- ROADMAP.md:271

## Объём и приёмка

Панель сертификата (Ctrl+Shift+C) врёт на каждом сайте: «No CT evidence found» при 2–3 SCT в листе — KNOWN_LOG_IDS пуст (tls/ct_logs.rs:33), evaluate_ct считает только известные журналы (ct.rs:303), текст fingerprint.rs:225; «No revocation info (no stapled OCSP)» даже при stapled Good — lib.rs:1492 жёстко передаёт OcspVerdict::Unknown. Неверны CAPABILITIES.md:232 («real revocation/CT rows») и note P3-tlshard («CT log enforcement завершена»). Исправить: текст «N SCT (журналы не сверяются)», без «no stapled OCSP» при неизвестном статусе, поправить доки. Приёмка: тест — 3 SCT неизвестных журналов → ct_status содержит «3». Источник: docs/research/site-compat/report.md §7.5 П1

## Источник

Найдено при сверке задач по исследованиям [`docs/research/`](../docs/research/README.md) (уровень 0 «сайт работает» и UX-мелочи), решение — [ADR-034](../docs/decisions/ADR-034-site-compatibility-strategy.md). Номера строк сняты на `main` 2026-10-12 — перед правкой перепроверить.
