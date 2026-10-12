# BUG-1614 — Идентичность браузера расходится между HTTP UA, navigator, userAgentData и Client Hints

**Статус:** OPEN
**Заведён:** 2026-10-12 (координационная сессия: сверка задач по `docs/research/` с кодом на `main`)
**Область:** network/js (`crates/network/src/http`, `crates/js/src/ua_client_hints.rs`, `navigator_bindings.rs`, `shim/web_api_shim_mid_b.js:746`)

## Симптом и место в коде

По умолчанию (HttpProfile::Chrome, crates/shell/src/config.rs:228) каждый слой называет себя по-своему. HTTP User-Agent: Chrome/153, из макроса chrome_major! (crates/network/src/http/mod.rs:43-59, headers.rs:166); H1, H2 и H3 берут один и тот же блок. navigator.userAgent: литерал 'Lumen/0.5.0' в окне (crates/js/src/shim/web_api_shim_mid_b.js:746) и второй такой же литерал в воркерах (crates/js/src/navigator_bindings.rs:270); у iframe свой peer-рантайм через тот же install_dom, поэтому там те же значения. navigator.appVersion обрезан до '5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36', без 'Chrome/153 Safari/537.36' (surface_api.rs:158, воркер navigator_bindings.rs:268). vendor = 'Google Inc.' (surface_api.rs:168), productSub = '20030107' (:186), platform = 'Win32' (navigator_bindings.rs:61): это значения Chrome, но они противоречат userAgent. navigator.userAgentData: статический Chrome 114, бренд 'Not A;Brand'/99 в старом формате GREASE, fullVersion 114.0.5735.133 (ua_client_hints.rs:32-52); в воркерах его нет вовсе. Sec-CH-UA, Sec-CH-UA-Mobile и Sec-CH-UA-Platform не отправляются никогда: client_hints_headers (crates/network/src/http/client_hints.rs:56-69) вызывается только из тестов, вне их есть лишь реэкспорт (http/mod.rs:27, lib.rs:280). Если эту функцию подключить, она отдаст '"Lumen/0.5.0"' не в формате structured-header-списка брендов. BiDi emulation.setUserAgentOverride меняет HTTP и navigator.userAgent окна, но не воркер (литерал), не appVersion, не userAgentData и не CH. Автоматизация сообщает 'Lumen/x' (driver/src/live_session.rs:348, bidi-server/src/protocol.rs:1076, devtools/src/cdp.rs:56), хотя в сеть уходит Chrome. Комментарий к профилю обещает «Matches current stable Chrome» (headers.rs:15), CAPABILITIES.md:196 числит Client Hints реализованными. ADR-007:66,102,111 требует 'Lumen/0.x' по умолчанию, а код после BUG-1113 перешёл к маске Chrome. Бисект BUG-1113: одно лишь добавление sec-ch-ua перевело accuweather, adobe и washingtonpost из 403/RST в 200, то есть отсутствие CH при UA Chrome — сигнал для Akamai. Chrome 114 в userAgentData отстаёт на 39 мажорных версий; это тот же класс, что 'Chrome/130' в BUG-1113. Это дефект: функциональность реализована, но слои противоречат друг другу и собственным заявлениям, а не отсутствуют.

## Где смотреть

- crates/shell/src/config.rs:228 http_profile: HttpProfile::Chrome (умолчание)
- crates/network/src/http/mod.rs:43-59 chrome_major!() = 153 -> CHROME_USER_AGENT
- crates/network/src/http/headers.rs:15 «Matches current stable Chrome», :166 UA Chrome
- crates/js/src/shim/web_api_shim_mid_b.js:746 userAgent: 'Lumen/0.5.0'
- crates/js/src/navigator_bindings.rs:268-270 воркер: appVersion обрезан, userAgent 'Lumen/0.5.0'
- crates/js/src/surface_api.rs:158 appVersion без Chrome/153; :168 vendor 'Google Inc.'; :186 productSub '20030107'
- crates/js/src/ua_client_hints.rs:32-52 бренды и fullVersion Chrome 114, статично
- crates/network/src/http/client_hints.rs:56-69 client_hints_headers: вне тестов вызовов нет, значение '"Lumen/x"'
- crates/js/src/worker.rs:463 воркер получает только worker_navigator_id_shim, userAgentData нет
- crates/js/src/v8_runtime/overrides.rs:47 override UA только для window navigator.userAgent
- crates/driver/src/live_session.rs:348, crates/bidi-server/src/protocol.rs:1076, crates/devtools/src/cdp.rs:56 сообщают Lumen/x
- CAPABILITIES.md:196 «Client Hints» в списке реализованного
- docs/decisions/ADR-007-anti-detection-stack.md:66,102,111 UA по умолчанию Lumen/0.x
- bugs/BUG-1113-FIXED.md бисект: «+ sec-ch-ua» -> accuweather/adobe/wapo 200

## Сюда же: Профили tor/firefox/safari: JS-идентичность не следует за HTTP-профилем

Не-Chrome профили меняют только HTTP-слой, JS-слой у них остаётся гибридом Lumen и Chrome. При --tor (crates/shell/src/cli_args.rs:727) HTTP отдаёт UA Firefox ESR 128 (http/mod.rs:94), а страница видит navigator.userAgent 'Lumen/0.5.0', vendor 'Google Inc.', productSub '20030107', appVersion в стиле Chrome и userAgentData Chrome 114. У Tor Browser и Firefox userAgentData нет, vendor '' и productSub '20100101'. При этом config.rs:263-283 закрепляет для Tor platform, языки и экран ради однородности, так что JS-однородность явно задумывалась, но UA-поля в неё не вошли. Firefox-профиль отдаёт UA под Linux X11 (headers.rs:200), Safari — под macOS (headers.rs:214), а navigator.platform остаётся 'Win32' (navigator_bindings.rs:61). Профиль Lumen отдаёт голое 'Lumen/0.5.0' без 'Mozilla/5.0' (http/mod.rs:33). LiveWindowSession::set_fingerprint_profile ничего не делает, а fingerprint_profile() всегда возвращает Standard (driver/src/live_session.rs:339-345), хотя ADR-007 обещает переключение на уровне контекста. Причина та же, что у B-IDENTITY (нет единого источника по HttpProfile), поэтому чинить это удобнее тем же источником.

## Объём и приёмка

Слои называют разный браузер: HTTP UA Chrome/153 (http/mod.rs:55, умолчание config.rs:228), navigator.userAgent 'Lumen/0.5.0' (web_api_shim_mid_b.js:746, воркер navigator_bindings.rs:270), appVersion без Chrome/153 (surface_api.rs:158), userAgentData Chrome 114 (ua_client_hints.rs:32-52, в воркере нет), Sec-CH-UA* не шлются (client_hints_headers только в тестах). Объём M: один источник от chrome_major! для UA/appVersion/userAgentData/Sec-CH-UA* с GREASE в окне, воркере, iframe и BiDi-override; поправить CAPABILITIES.md:196. Приёмка: тест-гейт ICR (эхо-сервер + navigator.* из 3 контекстов) — 0 расхождений. docs/research/site-compat/report.md §8.1 COMPAT-ID-01

## Источник

Найдено при сверке задач по исследованиям [`docs/research/`](../docs/research/README.md) (уровень 0 «сайт работает» и UX-мелочи), решение — [ADR-034](../docs/decisions/ADR-034-site-compatibility-strategy.md). Номера строк сняты на `main` 2026-10-12 — перед правкой перепроверить.
