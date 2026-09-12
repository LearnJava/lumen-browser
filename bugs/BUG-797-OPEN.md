# BUG-797 — `window.open()` возвращает нерабочую заглушку (нет `opener`, `postMessage` — no-op) — блокирует все WPT-тесты на базе `RemoteContext`/`dispatcher.js`

**Статус:** OPEN (ДОРАБОТКА → [GAP-NAVCTX](../ROADMAP.md))
**Тип:** нереализованная функциональность, не дефект реализованного кода — ведётся как задача `GAP-NAVCTX` в [ROADMAP.md](../ROADMAP.md), P3 как баг не берёт. Переклассифицировано 2026-09-02 ре-триажем пула WPT-RUN-5/6: срезы заводили багом всё подряд, потому что правила заведения ([docs/probe-method.md §8](../docs/probe-method.md)) тогда ещё не было. Файл сохраняет номер и путь — на него ссылаются CLAUDE.md, STATUS-файлы и python-тулинг, а запись наблюдений остаётся полезной там, где лежит.
**Заведён:** 2026-08-21 (WPT-RUN-6, срез 6 — разбор `html/semantics/embedded-content/bfcache`)
**Область:** `crates/js/src/dom.rs:11454-11483` (`window.open`)
**Владелец:** P1/P3 (движок). Заведён P2 в ходе WPT-задачи, здесь не чинится.

## Симптом

Все 6 файлов `html/semantics/embedded-content/bfcache/*.html` — TIMEOUT
(0/6 harness OK, `tests/wpt/run_report.py --all --root
html/semantics/embedded-content/bfcache --recursive`). Ни одной JS-ошибки в
логе — тест зависает молча.

## Причина (локализована чтением кода, не новым живым зондом)

`window.open()` (`crates/js/src/dom.rs:11454`) открывает новую ВКЛАДКУ на
уровне шелла (`_lumen_window_open`), но возвращает JS-стороне заглушку:

```js
return {
  closed: false,
  opener: null,
  name: target,
  location: { href: href, toString: function() { return href; } },
  close: function() { this.closed = true; },
  focus: function() {},
  blur: function() {},
  postMessage: function() {}   // <-- no-op, никогда не доставляет сообщение
};
```

Комментарий над функцией это прямо признаёт: «actual cross-window state
sharing is not implemented (window.opener is always null)». Это уже
отмечалось как известный хвост в [BUG-359](BUG-359-FIXED.md) («Второй
барьер не закрыт... `-late`-тесты (opener/popup round-trip) всё ещё
TIMEOUT, это отдельная задача»), но отдельного номера так и не получило —
заводится им сейчас.

WPT-инфраструктура `RemoteContext` (`/common/dispatcher/dispatcher.js` +
`resources/test-only-api.js`, вендорено) — стандартный способ, которым
сотни тестов (bfcache, popup/opener, cross-window messaging) управляют
второй вкладкой: `remote.execute_script(fn)` шлёт код через
`postMessage`/сервер-диспетчер и ждёт ответа тем же каналом. Раз
`postMessage` у возвращаемого объекта — no-op, любой `await
target.execute_script(...)` виснет навсегда: ни ошибки, ни отказа, чистый
TIMEOUT — ровно то, что видно на bfcache.

Конкретно для bfcache: `common.js`/`helper.sub.js` используют
`window.open(...)` как открытие исполнителя (`executor.html`) и затем ждут
ответа через `RemoteContextWrapper.execute_script` — первый же `await`
висит навсегда.

## Масштаб

Не только bfcache. Любой WPT-тест, использующий `RemoteContext`/
`dispatcher.js` (bfcache — 6 файлов только в этой подкатегории; та же
инфраструктура используется в `html/browsers/*`, `service-workers/*`,
многих cross-origin/popup тестах по всему корпусу — не подсчитано, оценка
масштаба вне скоупа этого среза) — заведомо TIMEOUT, а не FAIL, то есть
тратит полный бюджет ожидания на прогоне (дорого, см.
[docs/perf-method.md](docs/perf-method.md) и срез 15 WPT-RUN-5 про цену
TIMEOUT).

## Направление починки (не предписание)

`window.open()` уже открывает реальную вторую вкладку на уровне шелла
(`_lumen_window_open`) — недостающая часть чисто на JS/IPC-уровне: канал
`postMessage` между исходным окном и его `WindowProxy`-заглушкой, плюс
`opener` на новой вкладке, указывающий назад. Не обязательно решать это
как «полноценный второй JS-контекст с общей памятью» — WPT дисптчеру
достаточно рабочего `postMessage` в обе стороны (сообщения проксируются
через IPC-канал между вкладками, который уже существует для
`_lumen_window_open`).

## Как проверить фикс

1. Живой проб: `window.opener` на открытой через `window.open` вкладке
   указывает на исходное окно (не `null`), `postMessage` на объекте,
   возвращённом `window.open()`, реально долетает до new-tab's `message`
   listener.
2. WPT: `bfcache/*.html` — все 6 перестают быть TIMEOUT (`assert_bfcached`/
   `assert_not_bfcached` могут по-прежнему падать по существу — bfcache
   сам может быть не реализован, — но статус должен стать FAIL/PASS, не
   TIMEOUT).

## Дополнение (WPT-RUN-6, срез 28, 2026-08-23)

Замер `verify_window_history_jsurl_gaps.py --variant win-open-freeze` нашёл
барьер, стоящий **раньше** отсутствующего канала: после `window.open()`
документ-вызыватель перестаёт исполняться целиком — ни один его таймер,
поставленный до вызова, больше не срабатывает
([BUG-883](BUG-883-OPEN.md)). Пока это так, `opener`/`postMessage` у
заглушки чинить бессмысленно: ответ услышать некому.

Уточнение по самой заглушке из того же прогона: `w === window` — `false`,
`w.name` — переданное в `open()` имя, `w.location.href` — `about:blank` для
`open()` без аргументов, `w.document` — `undefined`, `w.focus`/`w.close` —
функции. В открытом документе `window.opener === null`.

## Срез 4 (GAP-NAVCTX, 2026-09-12, `p1-gap-navctx-srez4`)

Закрыт минимум из направления починки: `window.opener`/`postMessage` теперь
реально доставляют сообщение через границу вкладок в ОБЕ стороны, без
превращения в «второй JS-контекст с общей памятью» — ровно то, что карточка
и просила.

**Механизм.** Новый модуль `lumen_js::window_messaging` (см. его doc-комментарий
за полным разбором) — хаб, адресуемый *id вкладки*, а не указателем документа
(в отличие от `frame_bridge.rs`, чья модель не подходит: по ADR-016 M2.2 шелл
крутит ровно один `js_ctx` одновременно, все остальные вкладки простаивают в
`Lumen::bg_tabs`). `window.open()` возвращает `WindowProxy`-заглушку с токеном
(`_lumen_window_open` теперь возвращает `u32`) — токен нужен потому что шелл
создаёт настоящую вкладку только на следующем тике (`take_window_open_requests`),
а `.postMessage()` на заглушке может быть вызван раньше. `resolve_token`
(шелл, в момент создания вкладки) разрешает токен в реальный id и сливает то,
что уже успели поставить в очередь. Доставка — НЕ самопомпаж тикающего
контекста (как `_lumen_frame_pump_messages`), а прямой вызов
`PersistentJs::eval_js` шеллом на нужный (возможно, запаркованный) хэндл —
тот же приём, каким `switch_tab` уже прогоняет GC-паз на вкладке, ушедшей в
фон. `crates/shell/src/app/about_to_wait.rs` делает это каждый тик: для
активной вкладки и для каждой вкладки в `bg_tabs`.

**Что реально работает:** popup → opener (`window.opener.postMessage(...)`,
доставляется даже пока opener в фоне) и opener → popup (`w.postMessage(...)`,
включая случай «вызвано до создания вкладки»); `event.source` на обеих
сторонах — тот же самый JS-объект, а не заглушка (opener получает свой же
`w`, popup получает свой же `window.opener`).

**Известный остаточный хвост.** `_lumen_install_opener(ownId, openerId)` —
JS-глобали `_lumen_own_tab_id`/`_lumen_opener_tab_id` плюс сам объект
`window.opener` — вызывается шеллом ПОСЛЕ того, как `Lumen::navigate_to`
вернулся из создания вкладки, то есть после того, как отработали
синхронные (не отложенные) скрипты самого верха страницы. Синхронный
скрипт в `<script>` на самом верху попапа, вызывающий
`opener.postMessage(...)` до первой отдачи управления, всё ещё видит
`window.opener === null` (ровно как и раньше фикса) — это гонка, а не
доставка-в-никуда: `onload`/отложенные вызовы (частый случай и то, что
проверяет живой проб) отрабатывают верно. Закрыть до конца можно только
прокинув id вкладки через `install_dom` до его собственного вызова
`eval(WEB_API_SHIM)` — инвазивная правка сигнатуры с ~109 местами вызова,
вне бюджета среза.

Не тронуто (следующие срезы): BUG-883 (таймеры опенера в фоне) и BUG-797
сам (заглушка `opener` из среза 1 карточки выше — теперь это фактическая
проблема того же файла, не блокирующая эту доставку) остаются как были;
`RemoteContext`/`dispatcher.js`-тесты (см. «Масштаб» выше) не проверялись
живым пробом в рамках этого среза — WPT-прогон, если он покажет их всё ещё
TIMEOUT/FAIL по другой причине, заводится отдельно.

## Срез 5 (GAP-NAVCTX, 2026-09-12, `p1-gap-navctx-srez5`) — синхронный `opener.postMessage()` больше не видит `null`

Закрыт «известный остаточный хвост» среза 4 — но НЕ той правкой, которую
карточка предполагала. Threading id вкладки через `install_dom` (~109 мест
вызова, включая тестовые) остался неисследованным: вместо этого точка
установки `window.opener` подтянута из шелла (`about_to_wait`, после
`navigate_to`) в `run_scripts_with_dom` (`crates/shell/src/scripts.rs`) —
тот же файл, что уже делает несколько «one-shot push перед первой строкой
скрипта» (BUG-443 layout snapshot, CSSOM-7 stylesheet, TRUSTEDTYPES-1 CSP),
теперь и для opener.

**Механизм.** Новый однослотовый флаг в `window_messaging.rs`
(`arm_pending_opener`/`take_pending_opener`, `Option<(u32, u32)>`, НЕ карта
по id — шелл ведёт навигацию синхронно на одном потоке, так что между
`arm` перед `navigate_to` и `take` в самом первом `run_scripts_with_dom`
этой загрузки ничего чужого вклиниться не может). `about_to_wait.rs`
вооружает флаг непосредственно перед `self.navigate_to(source)` для
попапа; `run_scripts_with_dom` берёт его БЕЗУСЛОВНО в самом начале function
body (до обоих ранних `return` — иначе документ без скриптов, который
никогда не доходит до создания рантайма, оставил бы флаг висеть до
следующей, уже посторонней, загрузки) и, если пара есть, вызывает
`_lumen_install_opener(ownTabId, openerTabId)` сразу после CSP/TrustedTypes
push, перед циклом classic-`<script>`. Старый пост-факто вызов в
`about_to_wait.rs` (после `navigate_to` вернулся) оставлен как фолбэк —
идемпотентен (те же два id) и остаётся единственным путём для попап-документа
совсем без скриптов, который не создаёт рантайм вообще.

**Живая проверка** (MCP `--mcp-live-port`, `LUMEN_NO_ENGINE_THREAD=1`,
реальное окно, два `file://`-документа): попап с `<script>` на самом верху,
исполняющим `document.title = window.opener ? 'sync-has-opener' :
'sync-no-opener'` синхронно до первой отдачи управления, получил
`sync-has-opener` — до этого среза (по механизму, описанному в срезе 4)
результатом было бы `sync-no-opener`.

Гейт: `cargo build -p lumen-js -p lumen-shell --profile dev-release` чисто;
`cargo clippy -p lumen-js --features v8-backend --all-targets -- -D
warnings` и `cargo clippy -p lumen-shell --all-targets -- -D warnings` —
оба чисто; `cargo test -p lumen-js --features v8-backend window_messaging`
(6/6, включая новый `pending_opener_is_armed_once_and_cleared_on_take`) и
`cargo test --bin lumen scripts_and_frames` (84/84) — оба зелёные.
`scripts/scoped-test.sh` не запускался до конца — известная поломка
гейта ([BUG-805](BUG-805-OPEN.md), `lumen-network`), не от этой правки:
отдельный прогон `cargo test -p lumen-network --lib` в рамках гейта до
BUG-805 не дошёл (2207/2207 прошли за 4.37 с), полный скрипт просто не
успел завершиться за разумное время до конца работы над срезом.

Не тронуто: BUG-883 (таймеры опенера в фоне) и заглушка `opener: null` по
умолчанию для обычной навигации (не через `window.open()`) — как и раньше.
