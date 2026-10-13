# Механизмы совместимости у вендоров: интервенции, quirks, site patches, Compat Standard

Приложение к исследованию уровня 0 «Совместимость сайтов и действия пользователя, когда сайт работает не так». Основа — 30 находок (CM-01…CM-30) после проверки фактов: 19 подтверждены, 11 исправлены (в тексте только исправленные версии), опровергнутых нет. Отдельные детали, которые проверка не подтвердила, помечены «(не проверено)». CM-07 и CM-30 слиты в один блок. Состояние Lumen сверено с кодом и `CAPABILITIES.md` на 2026-10-12.

## Главное в области

1. **Большая часть интервенций чинит то, за кого сайт принимает браузер, а не рендеринг. При этом бэклог совместимости — это почти целиком баги движка.** У Firefox 235 из 450 записей о багах (52%) — «заблокирован» или «не поддерживается», и 169 из 210 файлов о блокировке исправлены подменой UA. В диагностированном бэклоге открыто 1191 platform-bug против 22 site-bug. Список правок закрывает дешёвую половину проблем, остальное — работа над движком. Для Lumen это две разные задачи с разными инструментами: идентичность и данные — против `BUGS.md` и `ROADMAP.md`.
2. **Индустрия пришла к одной архитектуре.** Это декларативная таблица «сайт → поведение из закрытого словаря», которая обновляется удалённо без релиза и выключается пользователем. Firefox доставляет её через Remote Settings за часы и принимает удалённо только JSON. WebKit 08.10.2026 сделал первый шаг к таблице, отдаваемой во время работы, но её пока никто не читает. Lumen может начать сразу с этой архитектуры и обойтись без исполняемого browser.js, на котором обжёгся Opera.
3. **Идентичность Lumen сейчас рассыпана на три версии.** Это дешёвый и срочный дефект. HTTP-заголовок — чистый Chrome/153 без токена Lumen, `navigator.userAgent` — литерал `Lumen/0.5.0`, `navigator.userAgentData` — Chrome 114, `navigator.vendor` — «Google Inc.», `window.chrome` нет. Firefox держит десятки скриптов, чтобы такие признаки на отдельных сайтах были согласованы, а антиботы ловят именно рассогласование. Профиль идентичности должен быть единым, с одним источником версии, переключаться атомарно и по сайту.
4. **Списки ведут даже браузеры на Chromium, и даже Safari дописывает к себе токен Chrome.** Сниффинг бренда не зависит от качества движка: у Opera на Chromium 21–30 патчей, у Edge — Domain Actions, у WebKit — «like Chrome/N.» для github.com. Рабочее умолчание — замороженная Chrome-форма с собственным токеном в конце, так поступают Edge и Ladybird. Полная подмена прячет движок от аналитики сайтов, и сайтам незачем под него тестировать.
5. **Реактивная модель для маленького браузера не работает.** Outreach слабеет даже у Mozilla: Akamai ответил примерно через 21 месяц, а метабаг о сниффинге Google открыт с 2014 года. Отчётов от пользователей при малой доле будут единицы. Нужны три вещи: интервенции как долгоживущий долг с тестами и сроком жизни, ночной краулер с поиском баннеров, WAF- и CAPTCHA-страниц и согласованный с заявленной идентичностью сетевой отпечаток.
6. **Самая дешёвая совместимость — стандартизированная: Quirks Mode, Compat Standard и нестандартные API из меты Mozilla 1170774.** Её надо закрыть до релиза. Российского сегмента в списках вендоров почти нет: 6 из 511 хостов у Firefox и ни одного у WebKit. Свой российский список и корпус — не опция, а ответственность Lumen.

## Находки

### Сводка: механизмы вендоров и что из них есть в Lumen

| Механизм | Firefox | WebKit / Safari | Браузеры на Chromium | Lumen (по коду на 2026-10-12) |
|---|---|---|---|---|
| Таблица правок по сайтам | 405 JSON-файлов, закрытый словарь действий | QuirkTable.json, 180 записей | Edge — Domain Actions; Opera — browser.js, 21–30 патчей | Нет |
| Удалённая доставка | Remote Settings, только данные, за часы | Первый шаг 08.10.2026, пока не читается | Edge — ECS | Нет |
| UA по умолчанию | Честный Firefox, точечные подмены | Честный Safari, точечные подмены | Chrome-форма + свой токен (Edge, Opera); чистый Chrome (Vivaldi) | HTTP — чистый Chrome/153 без токена (`crates/network/src/http/mod.rs:55`); JS — `Lumen/0.5.0` (`crates/js/src/shim/web_api_shim_mid_b.js:746`) |
| UA Client Hints | Не реализует | Не реализует | Бренд + «Chromium» + GREASE | JS — «Google Chrome»/«Chromium» 114 без Lumen (`crates/js/src/ua_client_hints.rs:32`); сборщик заголовка Sec-CH-UA с «Lumen/<версия>» есть (`crates/network/src/http/client_hints.rs:66`), но по grep вне тестов не вызывается |
| Переключение идентичности | По сайту, в записи интервенции | По сайту, в QuirkTable | Edge — по домену | Только глобальный `HttpProfile` (`crates/shell/src/config.rs:228`) и процесс-глобальная подмена через BiDi (`crates/shell/src/app/about_to_wait.rs:1138`) |
| Прозрачность | about:compat с выключателем | Тумблер Site-specific Hacks | edge://compat/useragent, флаг | Нет |
| Quirks Mode | 3 режима, 14 квирков | То же | То же | `DocumentMode` ставит парсер; часть квирков реализована (`crates/engine/layout/src/style/quirks.rs`, `crates/engine/layout/src/style/parse/color.rs:118`). `CAPABILITIES.md:38` пишет «detection only» — документ отстал |
| Compat Standard (-webkit-) | Полностью | Полностью | Полностью | Точечно: `-webkit-box` → flex, line-clamp, appearance, background-clip, text-size-adjust, user-select, `@-webkit-keyframes`, WebKitCSSMatrix. Простых алиасов вроде `-webkit-transform` grep не находит |
| Кнопка «сайт работает неправильно» | Report Broken Site с Firefox 123 | Feedback → Radar | Есть | Нет |

### 1. Что на самом деле чинят интервенции

**Firefox: 405 интервенций, больше половины багов — о том, за кого сайт принимает браузер (CM-01)** · массово · критично
- Что видит пользователь / что ломается: «браузер не поддерживается», пустая или урезанная страница, хотя движок с сайтом справился бы.
- Механизм: `browser/extensions/webcompat/data/interventions` в main на 12.10.2026 — 405 файлов. В них 450 записей в `bugs` (448 уникальных номеров) и 528 шаблонов в `matches` (517 уникальных, 511 уникальных хостов). Поле `issue`: firefox-blocked-completely — 187, unsupported-warning — 48, вместе 235 из 450 (52%); broken-interactive-elements — 43, broken-layout — 37, desktop-layout-not-mobile — 20. Подмена UA (`ua_string`) есть в 230 файлах, в 217 из них других действий нет. Из 210 файлов о блокировке 169 исправлены подменой UA. CSS-правки — в 97 файлах, JS — в 76. Платформы по интервенциям: all — 214, android — 104, desktop — 83 (по файлам: 212, 104, 77). В вики Mozilla механизм назван «pressure-release valve».
- Как решают другие: у Safari QuirkTable на 180 записей, у Edge — Domain Actions, у Opera Presto был browser.js (267 патчей в 2015 году). У Chrome собственного списка нет (CM-21).
- Что делать нам: строить слой в таком порядке: (1) идентичность по сайтам, (2) CSS, (3) JS. Каждую запись связывать с публичным багом и типом поломки из фиксированной таксономии (взять значения `issue` из Firefox). Начинать надо с идентичности: это самая массовая и дешёвая половина, а JS-патчи — самая дорогая и рискованная.
- Источники: https://github.com/mozilla-firefox/firefox/tree/main/browser/extensions/webcompat/data/interventions (405 файлов, 230 с подменой UA) · https://wiki.mozilla.org/Compatibility/System_Addon/Interventions

**Report Broken Site увеличил поток отчётов в 3,6 раза; диагностированный бэклог — почти целиком баги движка (CM-10)** · массово · высокая
- Что видит пользователь / что ломается: отчёт должен дойти до того, кто может исправить проблему. Чаще всего это команда движка, а не сайт.
- Механизм: Bugzilla, Web Compatibility :: Site Reports, создано по годам: 184 (2022), 387 (2023), 1392 (2024), 1178 (2025). Скачок совпал с кнопкой Report Broken Site в Firefox 123 (20.02.2024); рост 2024 года отчасти объясняется новым каналом. Открыто на 12.10.2026: Site Reports — 2034, Privacy: Site Reports — 640, needs-diagnosis — 939. По диагнозу: webcompat:platform-bug — 1191 открытых из 2715, site-bug — 22 из 30, chrome-bug — 3.
- Как решают другие: у WebKit отчёты уходят в закрытый Radar. На webcompat.com ML-бот автоматически закрывает невалидные отчёты.
- Что делать нам: встроенная кнопка отчёта, которая собирает скриншот, URL, консоль, список применённых правок и защит. Маршрутизация по диагнозу: движок — в бэклог движка (`BUGS.md`), сниффинг — в список идентичности, защита — в исключения. Главная метрика — закрытые баги движка, а не число правок. Отчёт уходит только по явному действию пользователя, так что с обещанием «без телеметрии» это совместимо.
- Источники: https://bugzilla.mozilla.org/rest/bug?product=Web%20Compatibility&component=Site%20Reports&chfield=%5BBug%20creation%5D&chfieldfrom=2024-01-01&chfieldto=2024-12-31&count_only=1 (1392 за 2024) · https://bugzilla.mozilla.org/rest/bug?keywords=webcompat%3Aplatform-bug&resolution=---&count_only=1 (1191 открытых) · https://www.ghacks.net/2024/02/20/firefox-123-is-out-with-broken-site-reporting-tool/

**WebKit: 180 квирков, 25 подмен UA и токен «like Chrome/N.» (CM-04)** · часто · высокая
- Что видит пользователь / что ломается: даже Safari на macOS часть сайтов отказывает или отдаёт урезанную версию, и он вынужден представляться другим браузером.
- Механизм: `Source/WebCore/page/QuirkTable.json` — 180 записей, 166 уникальных хостов, 164 разных поведения (на момент добавления таблицы — 163). Самые частые поведения:
  - NeedsUserAgentStringOverrideQuirk — 25 записей, все для macOS. Из них 21 подменяет UA на Chrome (18 — на Chrome/152, 3 — на Chrome/143), ещё 4 — на другую версию Safari (Version/18.6 ×3, Version/13.1.2 ×1).
  - ShouldDispatchSimulatedMouseEventsQuirk — 10.
  - NeedsScriptToEvaluateBeforeRunningScriptFromURLQuirk — 10.

  Отдельный механизм в `Quirks.cpp` превращает «like Gecko» в «like Gecko, like Chrome/N.»: для github.com — 151 (bug 319011 создан 09.07.2026), pinduoduo — 149, tiktok — 136. В `QuirkTable.cpp` 41 коммит с 27.08 по 09.10.2026: таблицу правят каждую неделю.
- Как решают другие: Firefox подставляет Chrome UA с токеном FxQuantum/<версия> (пресет Chrome_with_FxQuantum) и остаётся видимым в логах сайтов.
- Что делать нам: брать промежуточную идентичность — UA Lumen с совместимым токеном Chrome/NNN, а не полную подмену. Версию токена поднимать вместе с Chrome stable: устаревшая версия сама вызывает баннер «обновите браузер». В Lumen мажорная версия уже вынесена в один макрос `chrome_major!` (`crates/network/src/http/mod.rs:43`, сейчас 153). Из него и должны браться JS-строка и UA-CH (см. CM-14).
- Источники: https://raw.githubusercontent.com/WebKit/WebKit/main/Source/WebCore/page/QuirkTable.json (180 записей) · https://bugs.webkit.org/show_bug.cgi?id=319011 · https://github.com/WebKit/WebKit/blob/main/Source/WebCore/page/Quirks.cpp

**У Chromium нет файла квирков: веб пишут под Chrome, а особые случаи делают только для доменов Google (CM-21)** · массово · высокая
- Что видит пользователь / что ломается: для пользователя эталон — «в Chrome работает», и любое расхождение с Chrome он считает виной браузера.
- Механизм: Den Odell (14.05.2026): «Chrome doesn't really need one… The web is already built for Chrome» и «the lists are getting longer». При этом встроенное расширение hangout_services (thunk.js, в публичном коде с октября 2013 года) отдаёт страницам *.google.com `cpu.getInfo` и загрузку CPU процессов браузера, GPU-процесса и вкладки. Заголовок X-Client-Data отправляется только на домены Google.
- Как решают другие: каждый браузер не на Chromium копит свой список — у Firefox 405 файлов, у WebKit 180 записей, у WebKitGTK отдельный `UserAgentQuirks.cpp`. Brave выключил hangout_services по умолчанию в июле 2024 года (#39664) и удалил компонент (#39660, 1.69).
- Что делать нам: заложить совместимость как постоянную статью бюджета, а не разовую задачу. В спорных местах эталон — Chrome на Windows, это и основная платформа Lumen. Привилегии для отдельных доменов не копировать.
- Источники: https://denodell.com/blog/browsers-treat-big-sites-differently · https://simonwillison.net/2024/Jul/9/hangout_servicesthunkjs/ · https://github.com/brave/brave-browser/issues/39664

**Knowledge Base: метабаги «корневая причина → сайты» для приоритизации движка (CM-19)** · часто · высокая
- Что видит пользователь / что ломается: сайт требует API, которого в браузере нет (WebUSB, SpeechRecognition, setSinkId), и просто отказывает. Пользователь не понимает почему.
- Механизм: в компоненте Knowledge Base 125 открытых записей. Отчёты сайтов привязаны к ним в основном через поле `blocks`, поэтому рейтинг по одним `depends_on` и `see_also` искажён. С учётом всех связей лидеры такие: Unsupported Browser (1886128) — 341 блокируемый баг; Unsupported browser warning (1886129) — 234; Akamai — 30 (всего 36 связей); SpeechRecognition — 22 (35); расширение файла при скачивании — 33 связи; `::-webkit-scrollbar` — 27; WebUSB — 17; font inflation — 16; Google UA sniffing — 16 зависимостей; font-weight по сравнению с Chrome — 12 (16). Две первые строки — опять идентичность (CM-24).
- Как решают другие: Chrome приоритизирует по use counters (chromestatus), у WebKit приоритизация закрытая.
- Что делать нам: вести карту «возможность → затронутые сайты → суммарный impact» и брать по ней задачи движка (в `ROADMAP.md` и `BUGS.md`). Связи считать по всем полям. Для неподдерживаемых API показывать честное «сайт требует X, в Lumen его нет».
- Источники: https://bugzilla.mozilla.org/rest/bug?product=Web%20Compatibility&component=Knowledge%20Base&resolution=--- (125 открытых; 341 и 234 бага у двух метабагов Unsupported Browser)

### 2. Идентичность браузера: UA, Client Hints, JS-признаки

**Chrome-токены сразу открыли Ladybird Google Search, Gmail и Instagram; Servo выдаёт себя за Firefox (CM-06)** · массово · критично
- Что видит пользователь / что ломается: новому движку с честным UA отдают урезанный интерфейс, «not supported», throttling и 403. В урезанном Gmail половина интерфейса не нажималась.
- Механизм: Ladybird (рассылка за январь 2026) добавил `Chrome/140.0.0.0` (PR #7493), а затем `AppleWebKit/537.36 … Safari/537.36` (PR #7626), сохранив свой токен: «not trying to hide, but we do need to play the game». По описанию #7626, Google, Gmail и Instagram заработали только после токенов AppleWebKit и Safari — одного Chrome/N не хватило. В меню Debug есть Spoof User Agent и Navigator Compatibility Mode; PR #12319 (01.10.2026) сохраняет этот выбор между запусками. Servo в `components/config/prefs.rs` под Windows отдаёт «(Windows NT 10.0; Win64; {ARCH}; rv:153.0) Servo/… Firefox/153.0»; на других ОС строка своя.
- Как решают другие: Edge — Chrome UA плюс `Edg`; Vivaldi — чистый Chrome UA; Firefox по умолчанию честен и подменяет точечно (230 файлов).
- Что делать нам: UA по умолчанию — полная замороженная Chrome-форма (`AppleWebKit/537.36 (KHTML, like Gecko) Chrome/N.0.0.0 Safari/537.36`) с токеном Lumen в конце. При малой доле в allowlist'ы сайтов не попасть, а реактивный список не догонит длинный хвост. Для отдельных сайтов — переключатель идентичности с сохранением в профиле. Сейчас HTTP-заголовок Lumen — чистый Chrome/153 без токена (`crates/network/src/http/mod.rs:55`), то есть в логах сайтов Lumen не виден. Перед добавлением токена проверить, что WAF пропускают Chrome-форму с токеном в конце: alfabank.ru отвечал 403 на UA с одним незнакомым токеном (приложение 06).
- Источники: https://ladybird.org/newsletter/2026-01-31/ · https://github.com/LadybirdBrowser/ladybird/pull/7626 · https://github.com/servo/servo/blob/main/components/config/prefs.rs

**Сайты проверяют не только UA, но и window.chrome, navigator.vendor, userAgentData, InstallTrigger, платформу и формат версии (CM-14)** · часто · высокая
- Что видит пользователь / что ломается: подмена одного заголовка не помогает — JS всё равно определяет «не Chrome».
- Механизм: content scripts интервенций Firefox, по числу использований:
  - `define_minimal_window_chrome` — 9 (bankmandiri, oculus/meta, ferc.gov);
  - `use_chrome_vendor` («Google Inc.») — 8 (bankofamerica, f1tv, cbsnews);
  - `use_chrome_useragent` — 10;
  - `define_userAgentData` — 6 (digits.t-mobile, news.yahoo.co.jp);
  - `undefine_firefox_properties` и `undefine_InstallTrigger` — 3 + 2;
  - `spoof_as_windows` — 4;
  - `change_Firefox_to_FireFox` — 8 (сайты с регистрозависимыми регулярками: rakuten, simplenote);
  - `cap_Version_to_99` — santanderbank и группа «versions over 99».

  WebKit для части сайтов включает `ShouldExposeShowModalDialog`.
- Как решают другие: Ladybird ввёл Navigator Compatibility Mode, Firefox переключает признаки скриптами для отдельных сайтов.
- Что делать нам: сделать единый профиль идентичности. В него входят HTTP User-Agent, `navigator.userAgent`/`appVersion`/`vendor`/`platform`, `userAgentData`, Sec-CH-UA, наличие `window.chrome`, engine-специфичные глобальные объекты и сетевой отпечаток (CM-13). Переключать профиль атомарно и по сайту. Собственных уникальных глобальных объектов не вводить. Версию давать в Chrome-формате N.0.0.0.
  Сейчас профиль Lumen рассыпан:
  - HTTP — Chrome/153;
  - `navigator.userAgent` — литерал `'Lumen/0.5.0'` (`crates/js/src/shim/web_api_shim_mid_b.js:746`, в воркерах — `crates/js/src/navigator_bindings.rs:270`);
  - `appVersion` — обрезанная строка без `Chrome/N` (`crates/js/src/surface_api.rs:156`);
  - `userAgentData` — Chrome 114 (`crates/js/src/ua_client_hints.rs:32`);
  - `vendor` — «Google Inc.» (`crates/js/src/surface_api.rs:167`);
  - `window.chrome` на обычной странице нет: заглушка ставится только при `_LUMEN_EXTENSION_ACTIVE` (`crates/js/src/shim/web_api_shim_tail_b.js:6796`).

  Именно по такому рассогласованию антибот Ozon считает браузер подделкой (приложение 06). Первый шаг — выводить все поля из одного источника, где версия — `chrome_major!`.
- Источники: https://github.com/mozilla-firefox/firefox/tree/main/browser/extensions/webcompat/data/interventions (собственный разбор: 9/8/10/6 использований) · https://github.com/mozilla-firefox/firefox/blob/main/browser/extensions/webcompat/lib/ua_helpers.js

**UA Client Hints: сайты фильтруют по списку брендов, а предложение «Chrome UA для всех» в WHATWG не поддержали (CM-28)** · часто · высокая
- Что видит пользователь / что ломается: даже при Chrome-подобной строке сайт смотрит в `navigator.userAgentData.brands`, видит «не Chrome и не Edge» и отказывает (digits.t-mobile.com).
- Механизм: в Edge UA заморожен: Windows 11 отдаёт «Windows NT 10.0», версия в формате 120.0.0.0. Sec-CH-UA содержит «Chromium», «Microsoft Edge» и GREASE-бренд, данные высокой энтропии отдаются только по Accept-CH, и Microsoft советует сайтам проверять бренд «Chromium». В WHATWG html#10518 (25.07.2024, открыт, «needs implementer interest») предлагали отдавать всем замороженный Chrome UA, а настоящий — отдельным токеном. miketaylr ответил, что это «effectively possible via User-Agent Client Hints» и что браузер вправе отправлять «Google Chrome» вместе со своим брендом. Firefox подставляет `userAgentData` скриптом (`define_userAgentData` — 6 использований в 5 файлах).
- Как решают другие: Brave и Opera добавляют в UA-CH свой бренд; Firefox и Safari UA-CH не реализуют.
- Что делать нам: реализовать UA-CH с GREASE, «Chromium» и честным брендом Lumen, а «Google Chrome» добавлять только в профиле для отдельных сайтов. Версии в UA-CH и в UA-строке брать из одного источника. Сейчас в Lumen JS и сеть противоречат друг другу: JS-объект отдаёт «Google Chrome» и «Chromium» 114 без Lumen и с GREASE старого вида «Not A;Brand» (`crates/js/src/ua_client_hints.rs:32`). Заголовок Sec-CH-UA с брендом «Lumen/<версия>» собран (`crates/network/src/http/client_hints.rs:66`), но по grep вне тестов не вызывается.
- Источники: https://learn.microsoft.com/en-us/microsoft-edge/web-platform/user-agent-guidance · https://github.com/whatwg/html/issues/10518

**Баннеры «используйте Chrome»: вендоры прячут их интервенциями (CM-24)** · массово · критично
- Что видит пользователь / что ломается: «браузер не поддерживается» — пользователь уходит, хотя сайт часто полностью работает.
- Механизм: в Firefox действие `hide_messages` есть в 6 файлах: recochoku.jp, kaigetsu.staysee.jp, order.mealkeyway.com, gamma.app, interview.micro1.ai, wanyiwan.top. В recochoku и gamma скрывается контейнер с текстом «Chrome», при необходимости нажимается крестик. `hide_alerts` — ещё 4 записи. На встрече Mozilla 07.03.2023 сказали, что таких отчётов «doubled again this year», а причины бывают от WebUSB до «not testing in Firefox». В Knowledge Base у двух метабагов Unsupported Browser 341 и 234 связанных бага (CM-19). На форуме Opera пользователь объясняет часть патчей browser.js как раз скрытием подобных сообщений.
- Как решают другие: WebKit и Edge подменяют UA, Ladybird ставит Chrome-токены по умолчанию.
- Что делать нам: распознавать баннеры по русским и английским шаблонам («Используйте Google Chrome», «рекомендуем Яндекс Браузер», «Ваш браузер устарел», «browser is not supported»). Предлагать в один клик «Открыть как Chrome» с запоминанием для сайта, «Скрыть предупреждение» и «Сообщить». Если сайт действительно не работает (нужен WebUSB и т. п.), баннер молча не скрывать, а объяснить, чего не хватает.
- Источники: https://github.com/mozilla-firefox/firefox/tree/main/browser/extensions/webcompat/data/interventions (1902399-recochoku.jp.json, 1986438-gamma.app.json; 6 файлов hide_messages) · https://wiki.mozilla.org/Compatibility/Meetings/2023-03-07 («doubled again this year»)

**WebKitGTK: Chrome/300, Firefox/300 и подмена платформы на Macintosh; бренд в UA ломает Google (CM-05)** · часто · высокая
- Что видит пользователь / что ломается: Linux-браузер на редком движке блокируют сразу по двум признакам — по движку и по платформе (PayPal, WhatsApp, Chase, Atlassian).
- Механизм: в `platform/glib/UserAgentQuirks.cpp` три группы подмен:
  - `Chrome/300.0.0.0` — typekit, bankofamerica, docs.google, soundcloud (битый MSE), primevideo;
  - `Firefox/300.0` — bilibili (иначе только 720P), claude.ai, netflix, disneyplus, hbomax;
  - платформа Macintosh — yahoo, whatsapp, paypal, chase, outlook.office, atlassian.net, duckduckgo.

  По комментариям в коде, accounts.google.com при UA с брендом приложения отдаёт «ugly fallback login page», а docs и drive показывают предупреждение о неподдерживаемом браузере. Там же Chrome-UA назван «an invitation for sites to use JavaScript that works in Chrome that WebKit cannot handle».
- Как решают другие: в Firefox есть `change_OS_to_Windows` (go.idnow.de) и `spoof_as_windows` (4 сайта, в том числе keisan.nta.go.jp). Servo в сборке под Windows отдаёт Windows NT 10.0.
- Что делать нам: токен Windows NT 10.0 — самый безопасный, и это основная платформа Lumen, так что проблемы платформы у нас почти нет. Подменять по сайту лесенкой: сначала версия, потом Firefox-форма, потом Chrome-форма — последнюю только если движок точно выполнит Chrome-ветку кода сайта. Бренд Lumen заранее проверить на accounts, docs и drive.google.com. Профили Firefox и Safari в Lumen отдают Linux- и macOS-UA (`crates/network/src/http/headers.rs:200`, `:214`), хотя браузер работает на Windows. Если их оставлять, то только как часть атомарного профиля (CM-14).
- Источники: https://github.com/WebKit/WebKit/blob/main/Source/WebCore/platform/glib/UserAgentQuirks.cpp

**Списки ведут даже браузеры на Chromium: Edge выбрал Edg и держит Domain Actions, у Opera 21–30 патчей (CM-07, CM-30)** · часто · средняя
- Что видит пользователь / что ломается: движок полностью совместим, но браузер получает «не поддерживается» только из-за названия. Владелец сайта не может сам убрать свой домен из списка вендора.
- Механизм:
  - Edge. Токен `Edg` выбран «to avoid compatibility issues caused by Edge string»: ветки кода сайтов для «Edge» писались под EdgeHTML. Подмены UA (Domain Actions) получают только Beta и Stable. Microsoft параллельно связывается с владельцем сайта, а в документе политики подмены названы «intended to be temporary». Загрузкой списка управляет политика ExperimentationAndConfigurationServiceControl (ECS); прежняя EnableDomainActionsDownload работала в Edge 77–84 и устарела. Владельцу домена на Microsoft Q&A ответили: «The list is maintained by Microsoft and we can't edit it».
  - Opera на Chromium. «Active patches: 30» в 2016 году и 21 в мае 2022-го. Если сайт пускает Chrome, но не Opera, она представляется Chrome (патч «Mock as Chrome on popular American Bank Sites» в browser.js 2020 года). У Presto в 2015 году было 267 патчей. Около 7 записей у Opera на Blink в 2014 году — (не проверено).
  - Vivaldi отдаёт чистый Chrome UA: это общеизвестно, но источниками исследования не подтверждено.
- Как решают другие: у Firefox 230 файлов с подменой UA, на порядок больше, чем у Opera на Chromium. Эта разница — цена собственного движка.
- Что делать нам: ожидать у Lumen десятки записей из-за бренда и сотни из-за движка, а долю «бренд» сводить к минимуму удачным UA по умолчанию. Перед релизом проверить токен Lumen на коллизии с регулярками сниффинга и UA-парсерами (урок `Edg`). Механизм доменных переопределений UA нужен с первого дня. Владельцам сайтов дать публичный путь выйти из списка — у Microsoft его нет.
- Источники: https://learn.microsoft.com/en-us/microsoft-edge/web-platform/user-agent-guidance · https://learn.microsoft.com/en-us/answers/questions/334911/our-domain-is-listed-in-the-useragent-override · https://forums.opera.com/topic/16413/browser-js (30 и 21 активный патч)

**Pale Moon Native mode: новый токен движка даёт отказы библиотек (CM-26)** · редко · средняя
- Что видит пользователь / что ломается: с родным UA отказываются работать отдельные библиотеки и сервисы.
- Механизм: со строкой «Goanna/x PaleMoon/x» без «Gecko/20100101» не работают Google Charts (google-visualization-issues #2786, 2020, открыт). Что базы устройств принимали Goanna за телефон Wiko Goa и что режим совместимости оставлен по умолчанию «for a reason» — (не проверено: форум Pale Moon закрыт Anubis). Режимы Native, Gecko compatibility и Firefox compatibility описаны на palemoon.org.
- Как решают другие: Edge выбрал `Edg`, Ladybird окружил свой токен Chrome-токенами.
- Что делать нам: прогнать итоговую строку Lumen через ua-parser-js, device-detector, WURFL-подобные базы и регулярки GA и Яндекс Метрики. Строка должна определяться как десктопный Chrome-совместимый браузер на Windows. Проверку держать в CI и запускать при каждой смене UA.
- Источники: https://github.com/google/google-visualization-issues/issues/2786 · https://www.palemoon.org/support/prefs-advanced-general

### 3. Сетевой отпечаток и WAF

**WAF и бот-детекция CDN блокируют браузер с нестандартным отпечатком (CM-13)** · часто · критично
- Что видит пользователь / что ломается: «Access Denied», 403 или пустая страница (вход на account.lenovo.com, chewy.com, mexc.com). Выглядит как поломка браузера.
- Механизм: Mozilla об Akamai (bug 1903597) — «broken bot detection», 403 со ссылкой на errors.edgesuite.net и Reference вида «18.…». В bug 2025212 (user-impact-score 3701) предложили ловить errors.edgesuite.net/18 и вести use counter, по аналогии со счётчиком CAPTCHA из bug 1913421. Bug 1967922 — блокировка Akamai на account.lenovo.com. Бот-менеджеры сверяют UA с TLS ClientHello и HTTP/2 SETTINGS — это общеизвестная практика, но этими источниками напрямую не подтверждена (не проверено). При проверке фактов sberbank.ru отдал curl с Chrome-UA страницу WAF «user_blocked»: Chrome-строка поверх стека не от Chrome выглядит как бот. В Lumen TLS идёт через rustls. Эталон Chrome 130 (JA3/JA4) есть только в тестовых снапшотах (`crates/network/src/tls/fingerprint.rs`), а CBC-наборы, которые предлагает Chrome, rustls не поддерживает, так что точного совпадения с Chrome нет (не замерено).
- Как решают другие: Firefox расследует и добавляет телеметрию. У браузеров на Chromium отпечаток эталонный по определению.
- Что делать нам: включить сетевой отпечаток в профиль идентичности (CM-14) и согласовать его с заявленным UA. Замерить JA4 и HTTP/2 SETTINGS у Lumen против Chrome на стенде. Распознавать страницы блокировки (Akamai, Cloudflare, DDoS-Guard, Qrator) и объяснять: «сайт принял Lumen за бота» — с действием (сменить идентичность для сайта, сообщить). Вести локальный счётчик таких блокировок для отчёта и краулера.
- Источники: https://bugzilla.mozilla.org/show_bug.cgi?id=1903597 · https://bugzilla.mozilla.org/show_bug.cgi?id=2025212 (impact 3701) · https://bugzilla.mozilla.org/show_bug.cgi?id=1967922

### 4. Архитектура слоя правок: формат, доставка, жизненный цикл

**Декларативные интервенции с закрытым словарём; Firefox доставляет их через Remote Settings за часы (CM-02)** · часто · высокая
- Что видит пользователь / что ломается: поломка после обновления сайта или браузера не может ждать следующего релиза.
- Механизм: в записи Firefox есть `bugs` (с `matches` и `issue`), `platforms`, `min_version`/`max_version`, `only_channels` и `pref_check`. Действия берутся только из словаря:
  - пресеты `ua_string`: `add_Chrome`, `Chrome_with_FxQuantum`, `change_Firefox_to_FireFox`, `cap_Version_to_99`;
  - `hide_messages`, `modify_meta_viewport`, CSS `user_styles`;
  - content scripts из заранее вшитых файлов.

  `remote_settings_update_check.js` принимает удалённо только объекты interventions и shims, новый JS прислать нельзя. По вики, правка приходит «within a matter of hours… without requiring a Firefox dot release… or restart».
- Как решают другие:
  - WebKit. Коммит 323129@main (bug 326739, 08.10.2026) добавил QuirkTable.json как «first step toward a quirk table that can be served at runtime», но там же сказано: «Nothing reads it yet». Перечень поведений вынесен в QuirkBehaviors.yaml (с 01.10.2026, около 171–172 поведений, из них таблица использует 164).
  - Edge получает Domain Actions через ECS.
  - IE9 мог обновлять CV List ежедневно в виде XML («We can update the list daily»).
  - Opera рассылала исполняемый browser.js и проверяла обновления раз в неделю.
- Что делать нам: подписанный compat-манифест, в котором только данные и ссылки на встроенные, прошедшие ревью трансформации. У манифеста есть версия, откат и публичный журнал изменений, а раздаётся он с зеркала, доступного из РФ (приложение 06). Удалённо исполняемый код не присылать. Загрузка анонимная, без идентификаторов, поэтому с «без телеметрии» совместима.
- Источники: https://github.com/mozilla-firefox/firefox/blob/main/browser/extensions/webcompat/lib/remote_settings_update_check.js · https://commits.webkit.org/323129@main · https://wiki.mozilla.org/Compatibility/System_Addon/Interventions («within a matter of hours»)

**Жизненный цикл правок: 87% нынешних интервенций Firefox заведены после августа 2023 года (CM-23)** · часто · средняя
- Что видит пользователь / что ломается: если правки не удалять, список копит обходы для давно починенных сайтов и маскирует баги движка.
- Механизм: опорные номера багов: 1 850 000 — 24.08.2023, 1 900 000 — 31.05.2024, 1 950 000 — 23.02.2025, 2 000 000 — 13.11.2025. У 351 из 405 файлов (86,7%) номер ≥ 1 850 000. Номер бага — дата отчёта, а не правки, так что вывод о постоянном обновлении косвенный. Ключи жизненного цикла: `max_version` — 11 записей (значения 140, 142, 145, 148, 150; правка выключается сама), `min_version` — 16, `only_channels` — 15, `pref_check` — 16. Тесты на WebDriver (`mach test-interventions`) гоняют не реже раза за релиз. Сайты со входом, 2FA или блокировкой WebDriver вики называет «tricky (or impossible)», но и для них есть полуавтоматические тесты с примерами для логина, капчи и ручной 2FA.
- Как решают другие: в IE9 список держали «as small as possible», Opera удаляла патч по письму вебмастера, Edge называет подмены временными.
- Что делать нам: каждой правке — срок действия (версия движка или дата) с автоматической перепроверкой. Ночью гонять тесты «правка ещё нужна» и «правка работает», для сайтов со входом — полуавтоматические. Публиковать статистику добавленных и снятых правок. Снимать правку в том же коммите, где чинится баг движка: запись ссылается на `BUG-NNN`.
- Источники: https://github.com/mozilla-firefox/firefox/tree/main/browser/extensions/webcompat/data/interventions (351 из 405) · https://bugzilla.mozilla.org/rest/bug?id=1850000,1900000,1950000,2000000&include_fields=id,creation_time · https://wiki.mozilla.org/Compatibility/System_Addon/Interventions

**Риски интервенций: регрессии, широкие шаблоны, Chrome-код на чужом движке, невидимость в аналитике (CM-22)** · часто · средняя
- Что видит пользователь / что ломается: правка ломает другой сайт или перестаёт подходить после его обновления, и пользователь получает скрытую поломку.
- Механизм: в мае 2022 года сломанный browser.js Opera печатал версию на каждой странице, его откатили. WebKit bug 245612 «[Quirks] Site-specific hacks break native video controls on YouTube» (2022) до сих пор в статусе NEW. В текущем коде WebKit есть шаблоны вида `*://*.google.*/maps`, которые задевают чужие домены в зонах google.*. Из комментария WebKitGTK: Chrome-UA тянет код, который движок не выполнит. Подмена UA прячет движок от аналитики сайтов. Пресеты вроде `cap_Version_to_99` со временем устаревают сами.
- Как решают другие: Firefox пишет два теста на каждую правку и прогоняет их «at least once per Firefox release cycle». WebKit с 323129@main проверяет таблицу style-checker'ом.
- Что делать нам: только точные шаблоны хостов, без `*.google.*`. Привязывать правки к версии движка. Тесты «ещё нужна» и «работает» гонять на живых сайтах. При любой подмене оставлять токен Lumen. Предусмотреть мгновенное удалённое отключение отдельной записи. JS-трансформации ревьюить как код безопасности.
- Источники: https://bugs.webkit.org/show_bug.cgi?id=245612 · https://forums.opera.com/topic/55918/version-shown-in-pages · https://wiki.mozilla.org/Compatibility/System_Addon/Interventions

**Opera Presto browser.js: подписанные патчи раз в неделю, публичный список, outreach — и всё равно проигрыш (CM-08)** · часто · средняя
- Что видит пользователь / что ломается: годами сайты «не пускают Opera», а патчи сами вносят ошибки.
- Механизм: по странице 2005 года, файл был подписан, обновления проверялись раз в неделю, о каждом патче писалось в консоль («In most cases…»). Список сайтов был публичным (msdn, sony, faz.net), работали outreach-программа «Open the Web» и адрес web-compliance@opera.com. `override_downloaded.ini` — скачиваемый от Opera override.ini, в основном с подменой UA по доменам («User Agent|Spoof UserAgent ID»). В январе 2015 года browser.js для Presto содержал 267 активных патчей: обновления шли как минимум до 2015 года, хотя Opera перешла на Blink в 2013-м. Репозиторий github.com/operasoftware/browserjs, на который ссылалась страница 2014 года, сейчас отдаёт 404. В мае 2022 года сломанный browser.js в Opera на Chromium выводил версию на всех страницах.
- Как решают другие: Firefox взял у Opera список, тесты и удалённое обновление, но заменил исполняемый JS декларативным JSON.
- Что делать нам: site patching — клапан, а не стратегия. Монолитный JS-файл патчей не держать: правки изолированы по сайтам, у каждой свой автотест. Публичный архив правок хранить у себя, а не только во внешнем репозитории.
- Источники: https://web.archive.org/web/20050618004332/http://www.opera.com/docs/browserjs/ · https://forums.opera.com/topic/8285 (267 патчей) · https://forums.opera.com/topic/55918

**IE Compatibility View List: от 3122 сайтов в IE8 к трети в IE9 и узкие featureSwitch (CM-09)** · часто · средняя
- Что видит пользователь / что ломается: пользователь включал Compatibility View и получал эмуляцию старого IE, а разработчикам приходилось тестировать IE9 в нескольких режимах.
- Механизм: IE8 вышел со списком на 3122 сайта; около 2400 в pre-release — (не проверено). Для IE9 сайты проверяли перед внесением, и список в Beta стал «just over one thousand», то есть «a third of the size». Сайты отбирали по «site compat testing and internal usage», для RC/RTM — по данным селфхостеров. Microsoft могла обновлять список ежедневно («We can update the list daily») в виде XML. featureSwitch (например, `createElementWithMarkup`) включался «only for specific sites on the CV List», verizonwireless.com — лишь пример. От вендорского списка пользователь отказывался, сняв галочку «Include updated website lists from Microsoft». IEStandards.xml вместе с X-UA-Compatible убирал домен из личного списка пользователя (туда домен попадал по кнопке CV, хранился в HKCU\…\Browser Emulation), а не из списка Microsoft.
- Как решают другие: Edge оставил Enterprise Mode Site List для IE mode, только для корпоративных клиентов.
- Что делать нам: брать узкие переключатели конкретного API для сайта (как featureSwitch), а не версионные «режимы документа» — они умножают тестовую матрицу. Отказ от вендорского списка — одной галочкой.
- Источники: https://learn.microsoft.com/en-us/archive/blogs/ie/ie9s-faster-more-capable-compatibility-view-list (3122 → около 1000) · https://learn.microsoft.com/en-us/archive/blogs/ie/ie-compatibility-list-pruning · https://learn.microsoft.com/en-us/archive/blogs/ie/introducing-compatibility-view

### 5. Прозрачность и управление со стороны пользователя

**Прозрачность: about:compat, Site-specific Hacks, edge://compat, консоль Opera (CM-03)** · часто · высокая
- Что видит пользователь / что ломается: пользователь не знает, что браузер изменил сайт, и не может выключить правку, которая сама что-то сломала.
- Механизм:
  - Firefox. about:compat появился в Nightly в феврале 2019 года (bug 1488845), в релизе — с Firefox 68: список правок со ссылкой на баг и кнопкой Disable.
  - Safari. Переключатель Site-specific Hacks; Apple «strongly encourage developing and testing with Site-specific Hacks disabled».
  - Edge. Страница edge://compat/useragent и флаг `--disable-domain-action-user-agent-override`; в Canary и Dev подмен нет.
  - Opera (2005). Сообщения в консоли, публичный список сайтов, адрес web-compliance@opera.com, «Open the Web». Патчи отключались через Opera6.ini («Browser JavaScript» = 0). Проверка `operabrowserjs=no` в URL есть в самом browser.js (видна в версии для Opera на Chromium 2020 года).
- Как решают другие: аддон UA Status для Pale Moon раскрашивает иконку: синяя — подмены нет, фиолетовая — подмена от вендора, красная — пользовательская.
- Что делать нам: страница lumen://compat с правками, причиной, ссылкой на трекер, датой и переключателем. На самой странице — индикатор «применено исправление совместимости» с кнопкой «выключить и перезагрузить», рядом с панелью приватности в движковом chrome. Каждое срабатывание правки — запись в консоль DevTools. Dev-сборки по умолчанию без правок (как Canary и Dev у Edge), чтобы баги движка не маскировались. Это прямое продолжение позиционирования «прозрачный».
- Источники: https://www.ghacks.net/2019/02/28/firefoxs-new-web-compatibility-page/ · https://webkit.org/web-inspector/device-settings/ · https://learn.microsoft.com/en-us/microsoft-edge/web-platform/user-agent-guidance

**Пользовательские инструменты по сайту; глобальная подмена UA вредна (CM-25)** · часто · высокая
- Что видит пользователь / что ломается: пользователь хочет одну кнопку, а получает about:config или расширение-подменщик, которое ломает другие сайты.
- Механизм:
  - Opera Presto: в override.ini для домена задаётся почти любой параметр opera:config, включая маскировку под другой браузер.
  - Pale Moon: подмена UA для домена — `general.useragent.override.<домен>`. Глобальная подмена вынесена в `network.http.useragent.global_override`; если она задана, настройки показывают предупреждение (GUAOwarning) и кнопку сброса (GUAOReset). Оценка «(very) counterproductive» — (не проверено).
  - Ladybird: Spoof UA в меню Debug.
  - IE: кнопка Compatibility View.
  - Konqueror: идентификация по сайтам (не проверено).
- Как решают другие: в Firefox есть только about:compat, в Safari — Develop > User Agent для текущей вкладки.
- Что делать нам: панель «Совместимость» в информации о сайте. В ней:
  - идентичность (Lumen / Lumen в Chrome-форме / Chrome / Firefox), переключаемая атомарным профилем (CM-14);
  - выключатель интервенций;
  - отдельные защиты;
  - «перезагрузить и сравнить»;
  - «сообщить».

  Выбор запоминать для сайта и показывать иконкой, как UA Status. Глобальной подмены UA в UI не давать. Сейчас `HttpProfile` в Lumen глобальный (`crates/shell/src/config.rs:228`, по умолчанию Chrome), а подменить UA можно только процесс-глобально через BiDi (`crates/shell/src/app/about_to_wait.rs:1138`).
- Источники: https://gishiko.net/opera/wiki/override.html · https://www.palemoon.org/support/prefs-advanced-general · https://addons.palemoon.org/addon/ua-status/

### 6. Процесс: триаж, outreach, проактивный поиск

**При малой доле отчётов почти не будет — нужен проактивный обход сайтов (CM-29)** · массово · высокая
- Что видит пользователь / что ломается: Firefox получает 1200–1400 site reports в год (CM-10), у Lumen их будут единицы, и поломки на популярных сайтах останутся незамеченными.
- Механизм: список Firefox вырос из потока отчётов. 07.03.2023 Mozilla обсуждала автоматический поиск сообщений «unsupported». Счётчик CAPTCHA (bug 1913421) заведён 16.08.2024 и исправлен, счётчик блокировок WAF (bug 2025212) обсуждают в 2026 году. IE9 отбирал сайты тестированием и внутренним использованием, для RC/RTM — по данным селфхостеров.
- Как решают другие: Ladybird разбирает поломки крупных сайтов вручную. У Edge и Chrome сигнал идёт от огромной аудитории.
- Что делать нам: ночной краулер по топ-1000 глобальных и топ-500 российских сайтов плюс сценарии: вход, поиск, корзина, видео. Сравнение с Chrome по скриншотам, DOM, ошибкам консоли, HTTP-статусам, баннерам «не поддерживается», страницам WAF и CAPTCHA. Результаты — в тот же конвейер триажа. Строить на том, что уже есть: `scripts/perf_audit.py --mode compat` (свежий процесс на сайт) и BiDi-драйвер.
- Источники: https://wiki.mozilla.org/Compatibility/Meetings/2023-03-07 · https://bugzilla.mozilla.org/show_bug.cgi?id=1913421 · https://bugzilla.mozilla.org/show_bug.cgi?id=2025212

**Outreach почти не работает: ответа ждут месяцами, Google сниффит Firefox с 2014 года (CM-12)** · массово · высокая
- Что видит пользователь / что ломается: пока ждут ответа сайта, пользователь месяцами живёт с поломкой. Маленькому браузеру не ответят вовсе.
- Механизм:
  - Akamai (bug 1903597): outreach-contact-date — 2024-08-09, outreach-response-date — 2026-05-08, почти 21 месяц.
  - Метабаг Google UA sniffing (1985678) зависит от 16 багов. Самый старый — 975444 от 21.02.2014: Firefox на Android не получает tier1-выдачу Google.
  - Gmail (1854632): контакт 2025-02-03, ответ 2025-08-26.
  - Calendar «not supported» (2013911): контакт 2026-03-13, ответ 2026-04-15.

  На встрече 07.03.2023 outreach назвали «feels less and less effective», а баннеры, по словам участников, «doubled again this year».
- Как решают другие: Firefox и WebKit выпускают интервенцию, не дожидаясь сайта.
- Что делать нам: планировать интервенции как долгоживущие — с тестом и сроком (CM-23). Outreach вести адресно, к ключевым российским сайтам (рынок локальный, договориться реально), и замерять время до ответа.
- Источники: https://bugzilla.mozilla.org/show_bug.cgi?id=1903597 (21 месяц) · https://bugzilla.mozilla.org/show_bug.cgi?id=1985678 (16 багов с 2014) · https://wiki.mozilla.org/Compatibility/Meetings/2023-03-07

**Конвейер триажа Firefox — конечный автомат на ключевых словах (CM-11)** · часто · средняя
- Что видит пользователь / что ломается: без формального процесса отчёты оседают, и непонятно, кто и когда должен связаться с сайтом.
- Механизм: статусы — ключевые слова `webcompat:*`. Основная цепочка: needs-diagnosis → platform-bug / site-bug / ua-sniffing / tracker-blocking → needs-contact → contact-ready → contact-in-progress → site-wait → contact-complete. Отдельная ветка: needs-sitepatch → sitepatch-applied. Сейчас открыто: needs-contact — 443, contact-in-progress — 133, site-wait — 63, needs-sitepatch — 92, sitepatch-applied — 446 (из 652 всего). В user story бага хранятся user-impact-score, outreach-contact-date и outreach-response-date.
- Как решают другие: у Opera была программа «Open the Web», Microsoft вела outreach по CV List и Domain Actions, процесс WebKit закрыт.
- Что делать нам: публичный трекер Lumen с теми же статусами, impact-score и датами контакта и ответа. Статус «ждём сайт» показывать пользователю как известную проблему до того, как он на неё наткнётся. Формат (продолжение `BUGS.md` и `bugs/BUG-NNN-*.md` или метки в публичном трекере) — отдельное решение.
- Источники: https://bugzilla.mozilla.org/rest/field/bug/keywords · https://bugzilla.mozilla.org/show_bug.cgi?id=1903597

### 7. Совместимость на уровне движка: стандарты

**Compat Standard — чек-лист движка; -webkit-box помечен «not web compatible» (CM-16)** · массово · высокая
- Что видит пользователь / что ломается: без этого минимума тысячи сайтов теряют градиенты, анимации, flex и обводку текста.
- Механизм: compat.spec.whatwg.org (Last Updated 15.03.2026) включает:
  - `@-webkit-keyframes`;
  - медиапризнаки `-webkit-device-pixel-ratio` и `-webkit-transform-3d`;
  - четыре функции `-webkit-*-gradient()`;
  - около 57 простых алиасов свойств (семейства animation, transition, transform, flex, border-radius, mask и др.);
  - `-webkit-text-size-adjust`, `-webkit-text-fill-color`, `-webkit-text-stroke`;
  - `touch-action: pinch-zoom` и `manipulation`;
  - `WebKitCSSMatrix`, `window.orientation`;
  - раздел о структуре UA-строк Chrome, Firefox и Safari.

  Про `-webkit-box-*` там сказано: «known to not be web compatible [Issue #87]».
- Как решают другие: всё это есть во всех трёх движках, причём поведение `-webkit-box` расходится с текстом спецификации.
- Что делать нам: WPT `compat/` уже вендорен (`tests/wpt/compat/`) — включить его в регулярный прогон. Для `-webkit-box` и line-clamp эталон — Chrome. Раздел об UA-строках использовать как образец профиля идентичности. Что уже есть в Lumen по коду:
  - `display: -webkit-box` → flex (`crates/engine/layout/src/style/cascade.rs:1348`);
  - `-webkit-line-clamp`, `-webkit-appearance`, `-webkit-background-clip`, `-webkit-text-size-adjust`, `-webkit-user-select`;
  - `@-webkit-keyframes`;
  - `WebKitCSSMatrix` (`crates/js/src/shim/geometry_shim.js:610`).

  Простых алиасов вроде `-webkit-transform`, `-webkit-transition` и `-webkit-border-radius` grep по `crates/` не находит. Это надо подтвердить прогоном; если их нет — это первая по соотношению цены и эффекта задача движка в этой области.
- Источники: https://compat.spec.whatwg.org/

**-webkit- в Firefox: от списка сайтов (2014) к нативным алиасам (2016) и Compat Standard (CM-15)** · массово · высокая
- Что видит пользователь / что ломается: мобильный веб, особенно японский, выглядел в Firefox сломанным из-за CSS, написанного только с `-webkit-`.
- Механизм: 04.12.2014 появился CSSUnprefixingService для сайтов из «fixlist» (bug 1107378); запрос включить его для всех сайтов (1177263) закрыли как DUPLICATE. 07.10.2015 алиасы появились за префом (bug 837211), в релиз вошли с Firefox 49. Метабаг 1170774 перечисляет нестандартные API, реализованные ради совместимости:
  - 2015: `innerText` (ноябрь), `window.orientation` (октябрь);
  - без даты: `WebKitCSSMatrix`, `webkitMatchesSelector`, событие `webkitTransitionEnd`, `-webkit-text-fill-color` и `-webkit-text-stroke`, `-webkit-background-clip: text`, `webkitdirectory`;
  - 2017–2019: `-webkit-appearance` (04.2017), HLS на Android (04.2018), `webkitURL` через LegacyWindowAlias (07.2019).
- Как решают другие: Edge Legacy и Opera тоже реализовали `-webkit-`, итог закреплён в Compat Standard.
- Что делать нам: если шаблон встречается на тысячах сайтов, решать его в движке, а не списком. Список меты 1170774 пройти до публичного релиза. `webkitMatchesSelector` и `webkitURL` grep в JS-шимах Lumen не находит — сверить.
- Источники: https://bugzilla.mozilla.org/show_bug.cgi?id=1107378 · https://bugzilla.mozilla.org/show_bug.cgi?id=1170774 · https://hacks.mozilla.org/2016/09/firefox-49-fixes-sites-designed-with-webkit-in-mind-and-more/

**Interop 2025: общий балл вырос с 25–29 до 95–97; в Interop 2026 — 20 направлений (CM-18)** · массово · высокая
- Что видит пользователь / что ломается: сайты используют возможности из Interop без фолбэков, и движок без них ломается именно на новом.
- Механизм: Interop 2025 по данным WebKit: доля тестов, которые проходят во всех экспериментальных браузерах, выросла с 29% до 97%; все четыре браузера дошли до 99, Safari — с 43. По данным Mozilla, Firefox вырос с 46 до 99, а общий балл стабильных версий — с 25 до 95. Стартовый балл Chrome и Edge «около 80» — (не проверено). Interop 2026 — «20 focus areas (formed of 33 proposals)»; среди них Navigation API, anchor positioning и CSS zoom (с zoom у Gecko были проблемы на мобильных сайтах). Mozilla предупреждает: хороший балл возможен и при несогласованном поведении (anchor positioning, Navigation API). У Ladybird в январе 2026 года проходил 1 991 061 тест WPT.
- Как решают другие: Ladybird каждый месяц публикует прирост WPT.
- Что делать нам: направления Interop за последние два года — обязательная часть roadmap. Дополнительно мерить паритет с Chrome на реальных сайтах (краулер, CM-29): зелёные WPT не гарантируют совместимости.
- Источники: https://webkit.org/blog/17808/interop-2025-review/ (29% → 97%) · https://hacks.mozilla.org/2026/02/launching-interop-2026/ · https://raw.githubusercontent.com/web-platform-tests/interop/main/2026/README.md

**Quirks Mode: стандартный механизм, общий для всего движка, а не для отдельных сайтов (CM-17)** · часто · средняя
- Что видит пользователь / что ломается: старые страницы без doctype (сайты госорганов и вузов, внутренние порталы) разваливаются — таблицы, проценты, hex-цвета без «#».
- Механизм: quirks.spec.whatwg.org (Last Updated 30.09.2026) описывает 14 квирков (§3.1–3.13 и §4.1): hashless hex, unitless length, line-height, blocks ignore line-height, percentage height, заполнение viewport элементами html и body, ширина ячеек, nowrap min-width, collapsing table, text-decoration и цвет в таблицах, высота ячеек, `:active` и `:hover`. Режим (quirks, limited-quirks, no-quirks) выбирается по doctype. Долю страниц в quirks mode в источниках найти не удалось.
- Как решают другие: три режима есть во всех движках. Версионные document modes IE оказались тупиком (CM-09).
- Что делать нам: три режима строго по HTML и все 14 квирков с прогоном WPT `quirks/`. Режим определяется по doctype, а не по домену. В Lumen `DocumentMode` ставит парсер, и часть квирков уже работает: сброс шрифта в таблице, line-height и атрибут height (`crates/engine/layout/src/style/quirks.rs`), hashless hex (`crates/engine/layout/src/style/parse/color.rs:118`). При этом `CAPABILITIES.md:38` пишет «detection only» — документ отстал. Покрытие всех 14 квирков сверить.
- Источники: https://quirks.spec.whatwg.org/

### 8. Приватность и российский сегмент

**Российских сайтов в списках вендоров почти нет (6 из 511 хостов у Firefox, 0 у WebKit), а банки советуют «российский браузер» (CM-27)** · массово · критично
- Что видит пользователь / что ломается: готовых обходов для российских сайтов нет. Банки и Госуслуги вдобавок требуют доверия к российским корневым сертификатам.
- Механизм: в интервенциях Firefox ровно 6 российских доменов:
  - ozon.ru — Android, broken-videos, пресет `Chrome_with_FxQuantum`;
  - events.webinar.ru — blocked, `add_Chrome`;
  - prodamus.ru, anime-bit.ru, websdr.78dx.ru, nintendo.ru.

  Отдельно есть шим Rambler Authenticator. В QuirkTable WebKit доменов `.ru` нет. Сбербанк пишет: «В браузерах Opera, FireFox, «Mi Браузер» и некоторых других…» возможны ошибки, и «рекомендуем использовать российский браузер». Модель доверия Яндекс Браузера к НУЦ разобрана в приложении 06.
- Как решают другие: Яндекс Браузер работает на Chromium и встраивает российское доверие; Firefox и Chrome НУЦ не доверяют.
- Что делать нам: собственный корпус для РФ (Госуслуги, ФНС, Сбер, ВТБ, Т-Банк, Ozon, WB, Авито, VK, Mail, Яндекс, hh, Rutube) с ежедневным прогоном и своими правками. Российский список интервенций — целиком своя ответственность, чужого готового нет. Вопрос доверия к НУЦ решать явно и прозрачно (приложение 06).
- Источники: https://github.com/mozilla-firefox/firefox/tree/main/browser/extensions/webcompat/data/interventions (2031974-www.ozon.ru.json, 1898941-events.webinar.ru.json; 6 из 511 хостов) · https://www.sberbank.ru/ru/certificates

**Поломки от защиты приватности: 64 шима SmartBlock в Firefox и удалённый список исключений в Brave (CM-20)** · часто · высокая
- Что видит пользователь / что ломается: заблокированный трекер ломает вход через Google или Microsoft, видео, поиск — и пользователь выключает защиту целиком.
- Механизм: в `data/shims.js` 64 шима: GA и GTM, GPT, IMA, Facebook SDK, Microsoft Login, Rambler Authenticator, плейсхолдеры для Instagram, TikTok и Disqus. Метка tracker-blocking стоит на 1125 багах за всё время; Privacy: Site Reports — 640 открытых. Brave:
  - удалённый список Webcompat Exceptions Service, который выключает отдельные защиты на конкретных сайтах (#37074, 1.69.x);
  - UI с переключателями (#39510, 1.70.x);
  - удаление старого `shield_exceptions.cc` (#10598, 1.13.x);
  - пример: language farbling выключен на Albertsons и Safeway из-за бот-детекции Imperva (#55271).

  В QuirkTable WebKit есть `NeedsStorageAccessForYouTubeWatchLaterQuirk` и `NeedsPartitionedCookiesQuirk`.
- Как решают другие: в Safari ITP работает через квирки хранилища.
- Что делать нам: в отчётах разделять «сломала защита» и «сломал движок». Сделать шимы для топовых SDK, включая российские (Яндекс ID, VK ID, Метрика, Mail ID). Нужен удалённый список исключений по отдельным защитам и UI, где на сайте выключается одна защита, а не все. Подробно — в приложении 05.
- Источники: https://github.com/mozilla-firefox/firefox/blob/main/browser/extensions/webcompat/data/shims.js (64 шима) · https://github.com/brave/brave-browser/issues/37074 · https://github.com/brave/brave-browser/issues/55271

## Решения, которые должна принять команда

1. **UA по умолчанию.**
   - (а) Полная Chrome-форма с токеном Lumen в конце, как у Edge и Ladybird. Lumen виден в логах, большинство сниффинга проходит. Цена: нужен близкий к Blink набор API, иначе антиботы считают браузер подделкой; токен надо проверить на коллизии с UA-парсерами и WAF (CM-26, CM-07).
   - (б) Чистый Chrome без токена, как сейчас в HTTP-заголовке и у Vivaldi. Максимум допуска, но Lumen невидим для аналитики, сайтам незачем его тестировать, и это расходится с позиционированием «прозрачный».
   - (в) Честный Lumen-UA. Чисто, но сразу 403 и «не поддерживается» на части банков и сервисов (приложение 06).
   - При любом варианте: HTTP, `navigator.*`, UA-CH и отпечаток выводятся из одного источника (CM-14). Сейчас они противоречат друг другу, и это нужно чинить первым.
2. **UA Client Hints.**
   - (а) Реализовать с GREASE, «Chromium» и брендом Lumen, «Google Chrome» — только в профиле для отдельных сайтов. Правильно по WHATWG, но это работа и в сети, и в JS.
   - (б) Добавить «Google Chrome» по умолчанию (miketaylr считает это допустимым). Больше допусков, меньше честности.
   - (в) Не реализовывать вовсе, как Firefox и Safari: удалить нынешний `userAgentData` с Chrome 114. Дёшево, но часть сайтов отфильтрует браузер по брендам. Половинчатое состояние, как сейчас, — худший вариант.
3. **Формат и доставка правок.**
   - (а) Декларативный манифест с закрытым словарём и подписанной удалённой доставкой. Цена: сервер, ключи подписи, зеркало в РФ, процесс ревью.
   - (б) Правки только вместе с релизами. Дёшево, но поломки висят неделями.
   - (в) Исполняемый JS — отвергнуть (урок Opera).
   - Отдельно: идут ли трансформации JS (как `define_minimal_window_chrome`) в первую версию или только UA и CSS.
4. **Сетевой отпечаток.**
   - (а) Добиваться Chrome-подобного TLS и HTTP/2 под Chrome-форму UA. Возможно, придётся выйти за ограничения rustls — новая зависимость по ADR-027.
   - (б) Честный собственный отпечаток. Тогда Chrome-токены в UA ему противоречат, и бот-менеджеры это ловят.
   - Сначала замер: JA4 и HTTP/2 SETTINGS Lumen против Chrome, затем 403 на корпусе.
5. **Где живёт трекер совместимости и кто триажит.** Варианты: публичный трекер с метками по образцу `webcompat:*` или продолжение `BUGS.md`. Публиковать ли список правок и статистику. Кто отвечает владельцам сайтов.
6. **Краулер.** Объём (топ-1000 + топ-500 РФ + сценарии), откуда запускать (из РФ — иначе геоблоки), частота, эталон Chrome той же версии. Цена — машинное время и поддержка сценариев со входом.
7. **Кнопка отчёта.** Что собирать (скриншот, консоль, применённые правки и защиты), куда отправлять, сколько хранить, как сообщать пользователю об исправлении. Отчёт — только явным действием, вне телеметрии.
8. **Эталон для легаси-CSS.** Для `-webkit-box` и line-clamp — Chrome или текст спецификации. Очерёдность: Compat Standard, мета 1170774 и Quirks Mode — до или параллельно с направлениями Interop.
9. **Outreach.** Вести ли его вообще. Предложение: только к российским must-work сайтам и к вендорам UA-списков (1С, Контур, Яндекс 360), с замером времени ответа.
10. **Dev-сборки без правок по умолчанию** (как Canary и Dev у Edge). Плюс — баги движка не маскируются. Минус — тестировщики видят больше поломок, чем пользователи.

## Анти-цели области

- Не присылать удалённо исполняемый код. Канал правок передаёт только данные и ссылки на встроенные, прошедшие ревью трансформации.
- Не вести монолитный файл патчей, как browser.js. Каждая правка изолирована по сайту и имеет тест.
- Не делать версионные «режимы документа» и не привязывать квирк-режим к домену. Quirks Mode — только по doctype.
- Не давать пользователю глобальную подмену UA. Только идентичность по сайту, атомарным профилем.
- Не изображать Chrome в HTTP-заголовке, оставаясь Lumen в JS и Chrome 114 в UA-CH: это нынешнее состояние, его нужно убрать.
- Не прятать собственный токен полностью по умолчанию; при подмене для отдельного сайта сохранять токен Lumen.
- Не вводить собственные уникальные глобальные объекты и маркеры, по которым Lumen отличим от Chrome без пользы для сайтов.
- Не скрывать баннер «не поддерживается» молча, если сайт действительно не работает без отсутствующего API.
- Не держать правки без теста и срока жизни и не использовать широкие шаблоны хостов (`*.google.*`).
- Не копировать привилегии для отдельных доменов (hangout_services, X-Client-Data).
- Не делать outreach основным инструментом и не ждать ответа сайта, прежде чем выпустить правку.
- Не считать зелёные WPT и балл Interop доказательством совместимости без проверки на реальных сайтах.
- Не надеяться, что российские сайты покроют чужие списки: их там нет.
