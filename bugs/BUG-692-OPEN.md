# BUG-692 — CSP `upgrade-insecure-requests` directive is parsed but never consumed — has zero effect on network requests

**Статус:** OPEN
**Компонент:** network (`crates/network/src/csp.rs` — `CspPolicy::upgrade_insecure_requests`; enforcement gap spans `crates/network/src/lib.rs` `HttpClient::fetch*`/`mixed_content.rs`/`crates/storage/src/csp_policies.rs`)
**Найден:** P2, WPT-VENDOR-upgrade-insecure-requests, 2026-08-09

## Симптом

Категория `upgrade-insecure-requests` (`tests/wpt/upgrade-insecure-requests/`,
254 файла, пин `35be3b44`) — вендорена и прогнана целиком (`run_report.py
--all --root upgrade-insecure-requests --recursive --processes=4`, ~64 мин,
197 отобранных id) — **0/197 harness OK**, все TIMEOUT. Весь сигнал прогона —
100% реконфирмация уже задокументированного HTTPS-порт-гэпа (`UnknownIssuer`,
`docs/wpt-status.md:26`, `tests/wpt/certs/README.md`): каждый тестовый файл
категории — сам `.https.html` (top-level-навигация на `https://` происходит
до того, как страница вообще успевает загрузиться и подключить
`/common/security-features/resources/common.sub.js`, который отдельно тоже не
довендорен), поэтому TLS-хендшейк рвётся раньше любого JS. Новых номеров по
самому прогону не заведено — 197/197 идентичны по механизму отказа.

Проба source-level (grep по всему репозиторию) на месте — контейнер уровнем
выше, а не только сам прогон:

```
$ grep -rn upgrade_insecure_requests crates/
crates/network/src/csp.rs:119:    pub upgrade_insecure_requests: bool,
crates/network/src/csp.rs:132:            && !self.upgrade_insecure_requests
crates/network/src/csp.rs:188:            "upgrade-insecure-requests" => {
crates/network/src/csp.rs:189:                policy.upgrade_insecure_requests = true;
crates/network/src/csp.rs:346-348:  (только модульный unit-тест parse_upgrade_insecure_requests)
```

`upgrade_insecure_requests` — единственное поле `CspPolicy`, встречающееся
ровно один раз вне собственного файла `csp.rs` (в `is_empty()`-хелпере того
же файла) и нигде за пределами `csp.rs`: ни `HttpClient::fetch*`
(`crates/network/src/lib.rs`), ни `mixed_content.rs` (отдельный,
независимый от `CspPolicy` mixed-content классификатор — блокирует
http-подресурсы на https-странице, но никогда не *переписывает* их схему на
`https:`), ни `crates/storage/src/csp_policies.rs` (модульный doc-комментарий
там же прямо признаёт: «Phase 0: парсер директив + storage per-origin.
Реальное enforcement (отклонять fetch не из source-list) — отдельная задача,
требует hook в `HttpClient::fetch_with_redirect`») — не читает это поле.
Директива парсится в `CspPolicy`, но результат парсинга нигде не влияет на
исходящий запрос: ни один `http://`-URL подресурса на странице с
`Content-Security-Policy: upgrade-insecure-requests` никогда не переписывается
в `https://` перед отправкой.

## Масштаб

Затрагивает весь механизм директивы целиком — не частичный/краевой случай. Live-проба
через сборку не проводилась отдельно (source-level свидетельство однозначно:
`rg`/`grep` по всему `crates/` находит поле только внутри своего файла
определения), но это ожидаемо и для реального запроса: страница с `<meta
http-equiv="Content-Security-Policy" content="upgrade-insecure-requests">`
и `<img src="http://…">` на HTTPS-origin отправит запрос как есть (либо
заблокируется отдельным mixed-content классификатором по совершенно другой,
не связанной с этой директивой логике — см. `mixed_content.rs`, который сам
жёстко закодирован shell-ом в `MixedContentMode::SpecDefault` на каждой
навигации, независимо от того, объявила ли страница `upgrade-insecure-
requests` вообще).

`CAPABILITIES.md:165` перечисляет «CSP» в списке классификаторов сети как ✅
(`Origin/Mixed-Content/Sandbox/CSP/COOP классификаторы`) — формулировка верна
для mixed-content-классификатора (реально enforced) и вводит в заблуждение
для остального CSP: ни `upgrade-insecure-requests`, ни source-list-based
`connect-src`/`script-src`/… fetch-блокировка не подключены к
`HttpClient` (см. `csp_policies.rs`-докстринг выше — это признанный,
задокументированный, но нигде не отслеженный отдельным BUG-NNN пробел).
Данный баг заводится узко на `upgrade-insecure-requests`; общая CSP
source-list enforcement — отдельная, более крупная задача, не дублируется
здесь.

## Причина

`parse_csp_header` (`csp.rs:159` и далее) корректно распознаёт директиву
(`csp.rs:188-190`, юнит-тест `parse_upgrade_insecure_requests` проходит) и
сохраняет флаг в `CspPolicy`, но ни один вызывающий код (`HttpClient`,
navigation pipeline, `mixed_content.rs`) не читает `policy
.upgrade_insecure_requests` и не встраивает scheme-rewrite шаг в
fetch/navigate. Не регрессия — директива никогда не была подключена дальше
парсинга (Phase 0 по докстрингу `csp_policies.rs`).

## Дальше

Fix scope: перед выполнением subresource/navigation-запроса, если
per-origin `CspPolicy` (уже хранится в `csp_policies` storage) содержит
`upgrade_insecure_requests == true` и URL — `http:`/`ws:` (upgradeable
scheme по spec §4.1) на non-loopback host, переписать схему на
`https:`/`wss:` до отправки, аналогично тому, как Fetch spec требует делать
это до применения mixed-content классификации (upgrade имеет приоритет над
блокировкой — апгрейженный запрос уже не «mixed»). Естественная точка
интеграции — `HttpClient::fetch_with_redirect`, которую уже называет
докстринг `csp_policies.rs` как место для более широкого CSP enforcement;
реализовать здесь только scheme-rewrite шаг, не полный source-list fetch-блок
(отдельная задача).
