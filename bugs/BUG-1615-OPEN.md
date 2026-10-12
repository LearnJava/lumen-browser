# BUG-1615 — «Всё равно перейти» доступно на HSTS-хостах и при отзыве

**Статус:** OPEN
**Заведён:** 2026-10-12 (координационная сессия: сверка задач по `docs/research/` с кодом на `main`)
**Область:** network/shell (`tls/verifier.rs:107`, `page_load.rs:999`, `panels/cert_interstitial.rs:98`)

## Симптом и место в коде

proceed_cert_interstitial (crates/shell/src/page_load.rs:999-1011) вызывает lumen_network::tls::bypass::allow_host(host) и перезагружает страницу. Ни динамическое HSTS-хранилище (lumen_storage::shared_hsts_store, config.rs:322), ни список preload (hsts_preload.rs:100 is_preloaded) при этом не проверяются. Кнопку CertInterstitial::chrome_model показывает всегда (cert_interstitial.rs:98). LumenVerifier для хоста с обходом принимает любую ошибку цепочки (verifier.rs:107) и отозванный stapled OCSP (verifier.rs:116-117). RFC 6797 §8.4 и §12.1 требуют (MUST) разрывать соединение с Known HSTS Host при любой ошибке TLS без возможности обхода. ADR-034 §3: на банках и HSTS-хостах «Proceed anyway» не показывается. В брифе docs/tasks/ph3-tls-security-hardening.md HSTS не рассматривался. Ни в BUGS.md, ни в BUGS-FIXED.md такого бага нет. Дефект реализованного кода. Владелец указан P6, потому что P3 нет в списке ролей; по правилам проекта баги из BUGS.md берёт P3.

## Где смотреть

- crates/shell/src/page_load.rs:999-1011
- crates/shell/src/panels/cert_interstitial.rs:98
- crates/network/src/tls/verifier.rs:107
- crates/network/src/tls/verifier.rs:116-117
- crates/network/src/hsts_preload.rs:100
- crates/shell/src/config.rs:322
- docs/decisions/ADR-034-site-compatibility-strategy.md:78

## Сюда же: «Всё равно перейти» доступно на HSTS-хосте при ошибке сертификата

Дефект найден попутно при проверке экрана сертификата. CertInterstitial::chrome_model всегда отдаёт proceed_label «Всё равно перейти» (cert_interstitial.rs:98). proceed_cert_interstitial вызывает tls::bypass::allow_host (page_load.rs:1007). Дальше LumenVerifier для хоста из bypass принимает любую ошибку цепочки и даже отозванный по stapled OCSP сертификат (tls/verifier.rs:107, 117). HSTS при этом не проверяется, хотя хранилище и preload-лист есть (crates/network/src/hsts.rs, hsts_preload.rs:100 is_preloaded). По RFC 6797 §8.4 и §12.1 для Known HSTS Host ошибка TLS обрывает соединение без возможности обхода, Chromium кнопку не показывает. Проверено чтением кода, вживую не воспроизводилось. По правилам ролей это работа P3; P3 нет в списке владельцев, поэтому указан P6.

## Объём и приёмка

«Всё равно перейти» показывается и срабатывает на известных HSTS-хостах (включая preload) и на отозванном сертификате: proceed_cert_interstitial (shell/src/page_load.rs:999) пишет bypass без проверки HSTS-хранилища и hsts_preload.rs:100, LumenVerifier пропускает любую ошибку цепочки (verifier.rs:107) и Revoked (verifier.rs:116). Нарушает RFC 6797 §8.4/§12.1 (MUST, без обхода) и ADR-034 §3. Исправить: для HSTS-хоста и Revoked кнопку не показывать, allow_host не вызывать, bypass в verifier не применять. Приёмка: тест — preload-хост с битой цепочкой → chrome_model без proceed, bypass не записан; Revoked с bypass → ошибка. Источник: docs/research/site-compat/report.md §11 п.5

## Источник

Найдено при сверке задач по исследованиям [`docs/research/`](../docs/research/README.md) (уровень 0 «сайт работает» и UX-мелочи), решение — [ADR-034](../docs/decisions/ADR-034-site-compatibility-strategy.md). Номера строк сняты на `main` 2026-10-12 — перед правкой перепроверить.
