# BUG-953 — Document-Policy/Permissions-Policy никогда не генерируют violation-репорт: `ReportingObserver` есть, поставщика отчётов нет

**Статус:** OPEN (ДОРАБОТКА → [GAP-POLICYREPORT](../ROADMAP.md))
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

## Направление починки

Отдельная задача проектирования (`GAP-POLICYREPORT`): разбор заголовка
`Document-Policy`/`Permissions-Policy` (и его `-Report-Only` варианта),
таблица «фича → включена/отчёт-онли/выключена», точки проверки в местах,
где фича реально используется (`sync-xhr` — в `xhr.rs`, синхронный путь
`send()`), формирование и постановка в очередь `PolicyViolationReport` по
тому же контракту, что уже есть у `ReportingObserver`.
