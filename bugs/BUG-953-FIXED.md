# BUG-953 — Document-Policy/Permissions-Policy никогда не генерируют violation-репорт: `ReportingObserver` есть, поставщика отчётов нет

**Статус:** FIXED 2026-09-18 (P1, [GAP-POLICYREPORT](../ROADMAP.md)) — для найденного id (`sync-xhr`)
**Тип:** нереализованная функциональность, не дефект реализованного кода — та же форма, что [GAP-CSPENF](../ROADMAP.md) (BUG-811): интерфейс верхнего уровня (`ReportingObserver`) готов, но обнаружение и генерация нарушений политики — целая недостающая модель (парсинг заголовка политики, сверка фичи с политикой на каждом чувствительном вызове, формирование отчёта) — не один член.
**Заведён:** 2026-09-01 (WPT-RUN-6, срез 32, статическое чтение — грепом, без варианта в `verify_slice32_gaps.py`)
**Область:** js (`crates/js/src/reporting_api.rs` — класс `ReportingObserver` реализован; ни один Rust- или шим-файл нигде не конструирует отчёт с `type: 'document-policy-violation'`/`'permissions-policy-violation'`)
**Владелец:** нет (задача `GAP-POLICYREPORT` в `ROADMAP.md`, дорожка GAP).

## Симптом

`new ReportingObserver(cb, {types: ['document-policy-violation']}).observe()`
никогда не вызывает `cb` — нет ни одного источника, который бы поставил
отчёт этого типа в очередь. То же для `'permissions-policy-violation'`.
Страница, которая делает синхронный `XMLHttpRequest` (нарушение фичи
`sync-xhr` под report-only режимом Document/Permissions Policy) и ждёт
отчёта, зависает: `xhr.send()` в синхронном режиме отрабатывает штатно
(движок его не блокирует и не режектит), но обещанного отчёта не будет
никогда, потому что генерировать его некому.

## Прямое измерение

`grep -rn "document-policy-violation\|permissions-policy-violation"
crates/js/src/*.rs crates/js/src/shim/*.js` — ноль совпадений. Класс
`ReportingObserver` (`reporting_api.rs`) принимает и хранит колбэк с
фильтром по `types`, но очередь отчётов (`_lumen_reporting_queue` или
аналог) никогда не пополняется этими двумя типами ни из одного места —
не только для `sync-xhr`, а вообще ни для одной фичи Document/Permissions
Policy.

## Кого это держит

`document-policy/reporting/sync-xhr-report-only.html`,
`permissions-policy/reporting/sync-xhr-report-only.html` — оба ждут первый
отчёт (`await report`) и зависают. Вероятно шире (любой
`document-policy/reporting/*`/`permissions-policy/reporting/*` тест),
проверено только на этих двух id.

## Живой пробой (2026-09-04, WPT-RUN-6 срез 59)

Первое живое измерение — до этого находка была статической (грепом, без
прогона). `run_report.py --all --root document-policy/experimental-features
--recursive` через настоящий `wptrunner`+`wptserve` (не
`serve_wpt_like.py` — `network-efficiency-guardrails-json.tentative.html`
зависит от `?pipe=gzip`, которого этот скрипт не поддерживает,
`docs/probe-method.md`): все три файла каталога реально висят по 10 с
(`TEST_TIMEOUT`, ни одного PASS). Все три ждут один и тот же
`document-policy-violation` через `ReportingObserver`, что подтверждает
механизм этого бага на трёх новых id:
`network-efficiency-guardrails.tentative.html`,
`network-efficiency-guardrails-report-only.tentative.html`,
`network-efficiency-guardrails-json.tentative.html`. Классифицировано в
`timeout_audit.py` маркером `document-policy-violation-report-missing`.

## Направление починки

Отдельная задача проектирования (`GAP-POLICYREPORT`): разбор заголовка
`Document-Policy`/`Permissions-Policy` (и его `-Report-Only` варианта),
таблица «фича → включена/отчёт-онли/выключена», точки проверки в местах,
где фича реально используется (`sync-xhr` — в `xhr.rs`, синхронный путь
`send()`), формирование и постановка в очередь `PolicyViolationReport` по
тому же контракту, что уже есть у `ReportingObserver`.

## Исправлено (2026-09-18, P1)

Structured Fields-парсер `Document-Policy`/`-Report-Only`
(`crates/network/src/document_policy.rs`, `sf-boolean` `?0`/`?1`, RFC 8941
§3.2) плюс резолв disposition из обоих заголовков —
`page_source::document_policy_sync_xhr_disposition`/
`permissions_policy_sync_xhr_disposition` (`Permissions-Policy` через уже
существующий `crates/network/src/permissions_policy.rs`), enforce
приоритетнее report-only на каждом заголовке независимо. Проброшено через
`parse_and_layout` → `HttpClient::with_sync_xhr_policy` →
`JsFetchProvider::document_policy_sync_xhr_disposition`/
`permissions_policy_sync_xhr_disposition` → нативный биндинг
`_lumen_xhr_check_sync_policy`. Точка проверки — `xhr.rs::send()` перед
синхронной отправкой: под `enforce` бросает `DOMException('...',
'NetworkError')` (не блокирует запрос — не отправляет вовсе), под
`report`/`enforce` доставляет `document-policy-violation`/
`permissions-policy-violation` через `_lumen_deliver_report` с телом
`{featureId: 'sync-xhr', disposition, sourceFile, lineNumber, columnNumber}`.

Живой `run_report.py` подтвердил все 4 целевых WPT-файла зелёными:
`document-policy/reporting/sync-xhr-{report-only,reporting}.html`,
`permissions-policy/reporting/sync-xhr-{report-only,reporting}.html`
(раньше — TIMEOUT/висли на `assert_throws_dom` без отчёта). `cargo test -p
lumen-js --features v8-backend sync_xhr` 5/5, `cargo test -p lumen-shell
page_source`/`cargo test -p lumen-network document_policy` зелёные,
`cargo clippy --workspace --all-targets -- -D warnings` чист.

Вне скоупа: три id, найденных отдельным замером 2026-09-04
(`network-efficiency-guardrails*.tentative.html`) используют другую фичу
Document Policy без собственной точки проверки — заводятся отдельной
находкой, когда до них дойдёт очередь; таблица «фича → режим»
(`DocumentPolicy::feature_disabled`) уже общая инфраструктура для них.
