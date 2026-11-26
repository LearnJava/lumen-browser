# MCP-TOTAL — срезы

Директива пользователя 2026-11-26: MCP должен давать тотальный контроль над браузером, страницей и движком. Сейчас MCP-сервер (`crates/mcp/src/server.rs`) обобщён по трейту `BrowserSession` (`crates/driver/src/lib.rs:70`) и выставляет 20 инструментов (11 базовых + 9 `x-*`, `server.rs:203–528`) и 6 ресурсов (`:148–186`). При этом 19 команд `AutomationCommand` (из 35, `crates/driver/src/types.rs:531–692`) выставлены только в BiDi, а на живом окне 9 инструментов `x-*` и `resource://x-display-list` — заглушки (`crates/driver/src/live_session.rs:194–257`, `:389–402`). Указатели ниже сверены по `main 1b11a83cf6`; перед правкой строку перепроверить поиском.

Решения пользователя (2026-11-26; в срезах не пересматривать). (1) Новые инструменты — без `x-`, вровень с `click`/`type`; это отступление от ADR-024, его фиксирует срез S1c. (2) `x-` остаётся у существующих `x-*` и у новых форм данных, которые Lumen придумывает сам (ресурсы S5). Критерий: без `x-` — то, что повторяет уже работающую возможность и чью схему задаёт BiDi или существующая команда. (3) Имена — «глагол_существительное», как `list_tabs`/`switch_tab` (`server.rs:219–255`); клавиши — парой `key_down`/`key_up`. (4) Хвост BUG-338: `target` скролла носит вариант команды, а не `ScrollDelta`. (5) Честность headless: дефолты трейта возвращают `Err`, а не молчаливое `Ok`. (6) Этап S7 идёт до S6. (7) `network.continueWithAuth` в дорожку не входит: движковая часть — отдельная строка `HTTP-AUTH-LIVE`. (8) `set_viewport` — эмуляция viewport страницы, а не resize OS-окна. (9) S6 (настоящий OS-ввод) — только после отдельного решения пользователя; в очередь не поставлен.

## Общее для всех срезов

- **Форма среза:** вариант `AutomationCommand` (`types.rs:531`) + arm в `crates/shell/src/app/automation_cmds.rs` + метод трейта и его реализация в `live_session.rs` + инструмент(ы) в `server.rs` (`on_tools_list` — `:201`, `dispatch_tool` — `:672`) + проводка BiDi (`crates/bidi-server/src/protocol.rs`, таблица диспетчеризации `:671–720`), если у возможности есть BiDi-эквивалент. Новый вариант `AutomationCommand` — public-API изменение драйвера: проверить исчерпывающие `match` в потребителях, в том числе `crates/bidi-server/src/protocol/tests.rs`.
- **Ответ команды** — вариант `AutomationReply` (`types.rs:718–745`). Для структурированного ответа не переиспользовать `Eval(String)` (так уже делают `SetPermission` и `ComputedA11y`, `live_session.rs:464`, `:473`): добавить вариант-носитель JSON.
- **Схемы аргументов:** snake_case; `target` — четырёх форм, как у `click` (`docs/automation.md:106`, `server.rs:742` `parse_target`); координаты — CSS px контентной области.
- **Тесты, фиксирующие поверхность MCP** (`server.rs`): `tools_list_returns_twenty_tools` (`:1282`, ассерт `:1288`), `tools_list_marks_only_x_prefixed_tools_as_experimental` (`:1317`), `resources_list_returns_six_resources` (`:1243`). Срез, добавляющий инструменты или ресурсы, обновляет число и держит `experimental: true` только у `x-`.
- **Doc-sync тем же коммитом:** `docs/automation.md` §MCP (`:102–116`) и `CAPABILITIES.md:330` («Automation surfaces»).
- **Живая проверка.** Сборка — `dev-release`. Запуск: `./target/dev-release/lumen.exe --no-paint --viewport 800x600 --mcp-live-port <N> <url> >> .tmp/<log> 2>&1` (stderr — только в файл). Токен — в строке `[mcp] token:` журнала. Протокол — JSON-RPC построчно по TCP; первый запрос `initialize {"token": …}`, затем `tools/call {"name", "arguments"}`; перед первым `eval` дождаться `wait {"condition": "document_ready"}`. Образец клиента — `C:\kostja\Rust_Projects\Lumen_browser\.tmp\ua_probe.py` (поднимает свой экземпляр, шлёт команды, закрывает). Для срезов, где важен рендер (скриншот, display list, viewport, fullscreen), `--no-paint` не применять. Чужие `lumen.exe` не трогать; перед `scripts/scoped-test.sh` закрыть свои. Round-trip до окна — до 65 с (`DEFAULT_TIMEOUT`, `live_session.rs:45`): меньший клиентский таймаут — обрыв на стороне клиента, а не ответ.
- **Факт для проверок:** automation-клик даёт user activation (измерено 2026-11-26: в обработчиках `mousedown`/`click` `navigator.userActivation.isActive === true`), синтетический `dispatchEvent` из `eval` — нет.
- **Взятие и закрытие.** `bash scripts/task-claim.sh 6 mcp-total-s1a` (ветка `p6-mcp-total-s1a`). Закрытие — по `lumen-task-finish`: удалить указатель из `STATUS-P6.md`, строку в `ROADMAP.md` перевести в `done` (`python scripts/gen_roadmap.py`), раздел здесь пометить `## Status: MERGED` или удалить.
- **Если срез не влезает в сессию или вскрывается архитектурный выбор** — не тащить силой: закончить тем, что готово (WIP-коммит в ветку задачи), остаток нарезать (строки — в конец `ROADMAP.md`, указатели — в конец `STATUS-P6.md`) и сообщить пользователю.

## MCP-TOTAL-S1a: трейт `BrowserSession` — 10 методов и честные дефолты

**Status: MERGED** (2026-11-26, ветка `p6-mcp-total-s1a`): трейт расширен 10 методами с `Err`-дефолтами «только для живого окна», 8 `Ok`-дефолтов и 4 заглушки `LiveWindowSession` переведены на `Err`, тест на 18 `Err` у `InProcessSession` в `session/tests.rs`. Гейт: driver-тесты зелёные, clippy driver/bidi/mcp чист; полный `scoped-test.sh` красный вне диффа — BUG-1848.
- **Точка входа:** `crates/driver/src/lib.rs:70` (трейт; образец `Err`-дефолта — вкладки `:208–237`, текст вида «list_tabs: сессия без вкладок»; молчаливые дефолты — `:364–452`); `crates/driver/src/live_session.rs:61` (inherent `impl LiveWindowSession`: `pointer_move` — `:70`, `pointer_down` — `:78`, `pointer_up` — `:84`, `press_key` — `:91`, `type_focused` — `:97`, `get_cookies` — `:104`, `set_cookie` — `:112`, `delete_cookie` — `:118`, `add_preload_script` — `:129`, `remove_preload_script` — `:138`; остаются `new` — `:64` и `execute` — `:145`), `:153` (trait impl), `:415` (`set_fingerprint_profile` → `Ok(())`), `:529–538` (`set_clock`, `set_rng_seed`, `freeze_fingerprint` → `Ok(())`).
- **Сделать:** (1) в трейт — 10 методов с сигнатурами inherent-методов и дефолтом `Err(Error::Other("<метод>: только для живого окна (--mcp-live-port)".into()))`; (2) их тела перенести в `impl BrowserSession for LiveWindowSession`; (3) дефолты восьми методов (`set_offline` — `:364`, `set_timezone` — `:376`, `set_permission` — `:385`, `computed_a11y` — `:392`, `add_intercept` — `:408`, `remove_intercept` — `:415`, `resolve_intercepted_request` — `:429`, `poll_intercepted_requests` — `:439`) с `Ok(...)` — на тот же `Err`; (4) в живой сессии `set_fingerprint_profile`, `set_clock`, `set_rng_seed`, `freeze_fingerprint` — `Err` вместо `Ok(())`; геттер `fingerprint_profile` не трогать.
- **Не трогать:** `InProcessSession` (`crates/driver/src/session.rs:1027`) и `WinitSession` (`crates/driver/src/winit_session.rs:709`) — наследуют дефолты; `AutomationCommand`; логику BiDi (зовёт методы через трейт, импорт есть — `crates/bidi-server/src/protocol.rs:61`).
- **Готово, когда:** `cargo test -p lumen-driver` зелёный; новый тест: у `InProcessSession` каждый из 18 методов (10 новых + 8 бывших no-op) возвращает `Err`; `cargo check -p lumen-bidi-server -p lumen-mcp` чист. Потребителей no-op-дефолтов в production-коде нет (grep по `crates/`, `scripts/` без `tests/`, 2026-11-26) — тест, державшийся на `Ok`, переписывается.
- **Гейт:** `cargo clippy -p lumen-driver -p lumen-bidi-server -p lumen-mcp --all-targets -- -D warnings`; `cargo test -p lumen-driver`; `scripts/scoped-test.sh`.
- **Зависит от:** —

## MCP-TOTAL-S1b: MCP — инструменты ввода
- **Точка входа:** `crates/mcp/src/server.rs:201` `on_tools_list` (образец — `click`, `:269–282`), `:672` `dispatch_tool` (arm `click` — `:728`), `:34` `SERVER_INSTRUCTIONS`, тесты `:1227` (описание — законченное предложение), `:1282`/`:1288`, `:1317`.
- **Сделать:** шесть инструментов, все `experimental: false`: `pointer_move {x, y}`, `pointer_down {x, y}`, `pointer_up {}`, `key_down {key, code?}`, `key_up {key, code?}`, `type_focused {text}`. В описаниях схем зафиксировать: координаты — CSS px контентной области; кнопка только левая (у команды `PointerDown` поля `button` нет, `types.rs:564`); `key` — `Escape`, `Enter`, `Shift`…; `code` — `KeyA`, `ShiftLeft`…, необязателен (по умолчанию выводится из `key`); модификаторы arm распознаёт по `key` (`automation_cmds.rs:386`). Обновить `SERVER_INSTRUCTIONS`, ассерт числа инструментов (20 → 26), `docs/automation.md` §MCP, `CAPABILITIES.md:330` (пометка live-only).
- **Не трогать:** `click`, `type`, `scroll`; инструменты `x-*`.
- **Готово, когда:** `tools/list` = 26; живая проба: `pointer_move` на ссылку даёт `:hover` (`eval("document.querySelector('#a').matches(':hover')")` — `true`); `key_down`/`key_up` с `Escape` закрывают открытый `<select>`; `type_focused` после `click` по полю вводит текст; headless `--mcp-port`: все шесть отвечают `isError` с текстом «только для живого окна».
- **Гейт:** `cargo clippy -p lumen-mcp --all-targets -- -D warnings`; `cargo test -p lumen-mcp`.
- **Зависит от:** MCP-TOTAL-S1a.

## MCP-TOTAL-S1c: MCP — эмуляция, `computed_a11y`, запись в ADR-024
- **Точка входа:** та же, что в S1b; `docs/decisions/ADR-024-introspection-api-levels.md` (правило продвижения L1→L2 — строки 93–97, «no L2 before v1.0» — 124–138).
- **Сделать:** пять инструментов: `set_offline {offline: bool}`, `set_user_agent {user_agent}`, `set_timezone {timezone?}` (нет аргумента или `null` — сброс), `set_permission {name, state}` (BiDi-аргумент `origin` не поддерживается — override глобальный; допустимые значения — `crates/js/src/v8_runtime/overrides.rs:221`), `computed_a11y {selectors: string[]}` → `{role, name}` или `null`. Обновить `SERVER_INSTRUCTIONS`, ассерт (26 → 31), доки. В ADR-024 — запись решения пользователя 2026-11-26: инструмент, повторяющий работающую возможность, выставляется без `x-` и без `experimental`; критерий — из преамбулы этого брифа.
- **Готово, когда:** `tools/list` = 31; живая проба: после `set_user_agent` строка есть в `eval("navigator.userAgent")`; `set_permission` возвращает `true` для допустимого имени и `false` для неизвестного; `computed_a11y` для `<button>OK</button>` → `{role: "button", name: "OK"}`; после `set_timezone {timezone: "Asia/Tokyo"}` `eval("Intl.DateTimeFormat().resolvedOptions().timeZone")` — `Asia/Tokyo` (ожидание: arm ставит глобальный override и исполняет скрипт в странице, `automation_cmds.rs:619`; если движок отдаёт иное — разобраться, а не подгонять критерий).
- **Гейт:** `cargo clippy -p lumen-mcp --all-targets -- -D warnings`; `cargo test -p lumen-mcp`; `python scripts/check_doc_links.py`.
- **Зависит от:** MCP-TOTAL-S1a (порядок с S1b — любой).

## MCP-TOTAL-S1d: MCP — перехват, cookies, preload
- **Точка входа:** та же, что в S1b; типы ответов — `crates/driver/src/types.rs:177` `InterceptedRequest`, `:697` `AutomationCookie`.
- **Сделать:** девять инструментов: `add_intercept {id?, phases: string[], url_patterns: string[]}` (фазы — `beforeRequestSent`, `responseStarted`; `authRequired` не поддерживается — ошибка; `id` необязателен, по умолчанию генерируется сервером), `remove_intercept {id}`, `resolve_intercept {request_id, continue: bool}`, `poll_intercepts {}` → `[{request_id, url, phase, status}]`, `list_cookies {}`, `set_cookie {name, value, domain, path, secure, http_only, same_site, expiry}` (поля 1:1 `AutomationCookie`, плоско), `delete_cookie {name, domain, path}`, `add_preload_script {id?, function_declaration}`, `remove_preload_script {id}`. Обновить `SERVER_INSTRUCTIONS`, ассерт (31 → 40), доки; заодно в `docs/automation.md:104` «All 8 legacy tools» → 11 базовых.
- **Готово, когда:** `tools/list` = 40; живая проба: `set_cookie` → `list_cookies` содержит cookie → `eval("document.cookie")` содержит её имя; `add_intercept` с шаблоном адреса локальной страницы → `navigate` на неё → `poll_intercepts` возвращает запрос → `resolve_intercept {continue: true}` → страница загружается; `add_preload_script` с функцией, пишущей `window.__pre = true`, → после `navigate` `eval("window.__pre")` — `true` (preload исполняется в каждом документе — BUG-1811).
- **Гейт:** `cargo clippy -p lumen-mcp --all-targets -- -D warnings`; `cargo test -p lumen-mcp`.
- **Зависит от:** MCP-TOTAL-S1a.

## MCP-TOTAL-S1e: `scroll` с target (хвост BUG-338)
- **Точка входа:** `crates/driver/src/types.rs:550` `Scroll(ScrollDelta)`; `crates/driver/src/live_session.rs:345` `scroll` (роняет `_target`); `crates/shell/src/app/automation_cmds.rs:222–224` (arm крутит только страницу); образец — `crates/driver/src/session.rs:1270–1299`; `crates/shell/src/lumen/automation.rs:50` `resolve_automation_target`, `:524` `scroll_by_delta`; схема и arm MCP — `crates/mcp/src/server.rs:304–328`, `:758–780`; `crates/bidi-server/src/protocol/tests.rs:49`.
- **Сделать:** `Scroll { delta: ScrollDelta, target: Option<Target> }` (решение №4; `ScrollDelta` не менять); живая сессия передаёт `Some(target)`. В arm: при `Some(target)` — резолв цели → `lumen_layout::find_scroll_container_for_node` → скролл контейнера тем же путём, каким его скроллит настоящий `MouseWheel` (путь развести), fallback — `scroll_by_delta`. Схема MCP `scroll`: `target` — `["string", "object"]`, как у `click`/`type` (`server.rs:276–277`).
- **Не трогать:** страницу-скролл без target; `ScrollDelta`.
- **Готово, когда:** живая проба: `scroll {target: "#box", delta: {x: 0, y: 200}}` на `<div id="box" style="overflow: scroll; height: 100px">` меняет `eval("document.querySelector('#box').scrollTop")` и не меняет `window.scrollY`; без target и с `target: "body"` — как раньше.
- **Гейт:** `cargo clippy -p lumen-driver -p lumen-shell -p lumen-mcp -p lumen-bidi-server --all-targets -- -D warnings`; `scripts/scoped-test.sh`.
- **Зависит от:** MCP-TOTAL-S1a.

## MCP-TOTAL-S7a: JS-диалоги — `user_prompt` и `handle_user_prompt`
- **Точка входа:** `crates/shell/src/lumen/dialogs.rs:42` `poll_page_dialog`, `:62` `page_dialog_model`, `:121` `answer_page_dialog`, `:239` `handle_page_dialog_key`; BiDi — `crates/bidi-server/src/protocol.rs:545` `fire_user_prompt` (без вызывов), `:2038` `bc_handle_user_prompt` (всегда «no such alert»).
- **Сделать:** `AutomationCommand::UserPrompt` (какой диалог открыт: тип, текст, значение по умолчанию) и `HandleUserPrompt {accept, user_text}` + варианты ответа. Arm'ы не должны ждать JS-поток: пока диалог открыт, он заблокирован (`CAPABILITIES.md:300`). MCP: `user_prompt {}` → `{open, type, message}`; `handle_user_prompt {accept, user_text?}`. BiDi: `fire_user_prompt` при открытии диалога, `browsingContext.handleUserPrompt` — в живое окно. Headless не менять: там `alert` принимается, `confirm`/`prompt` отвечают `false`/`null`.
- **Готово, когда:** живая проба: страница с `confirm("go?")` — `user_prompt` отвечает `open: true, type: "confirm"`; `handle_user_prompt {accept: true}` возвращает управление, `eval("window.__answer")` — `true`; без диалога `handle_user_prompt` отвечает ошибкой «диалог не открыт»; для `prompt` текст `user_text` доходит до страницы; BiDi `browsingContext.handleUserPrompt` на открытом диалоге больше не отвечает «no such alert».
- **Гейт:** clippy по затронутым крейтам (`lumen-driver`, `lumen-shell`, `lumen-mcp`, `lumen-bidi-server`) + `scripts/scoped-test.sh`.
- **Зависит от:** MCP-TOTAL-S1b (паттерн инструментов).

## MCP-TOTAL-S7b: файловый input — `set_files`
- **Точка входа:** `crates/shell/src/lumen/file_picker.rs:18` `open_file_picker`, `crates/js/src/file_input.rs` (токены регистрирует шелл через `register_file_token`; `:571` `__lumen_C._lumen_file_lists[nid]`), `crates/shell/src/platform/file_dialog.rs:34`; BiDi — `crates/bidi-server/src/protocol.rs:2648` `input_set_files` (validate-then-ACK).
- **Сделать:** `AutomationCommand::SetFiles {target, paths}` — доставка без OS-диалога: для каждого пути `register_file_token`, затем тот же путь, что у пикера, плюс события `input`/`change`. MCP: `set_files {target, paths: string[]}`. BiDi: `input.setFiles` — в живое окно.
- **Не трогать:** модель безопасности токенов — JS не видит путей, читает только зарегистрированные файлы (`file_input.rs:1–15`).
- **Готово, когда:** живая проба: после `set_files` у `input` `files.length` равно числу путей, `files[0].name` совпадает с именем файла, обработчик `change` вызан; несуществующий путь — ошибка; `eval("input.files[0].text()")` (через два `eval`, промис MCP не ждёт до S7f) возвращает содержимое.
- **Зависит от:** MCP-TOTAL-S1b.

## MCP-TOTAL-S7c: загрузки
- **Точка входа:** `crates/shell/src/download.rs:256` `start_download`, `:300` `cancel`, `:355` `start_url_download`, `:369` `poll`, `:441` `entries`; BiDi — `crates/bidi-server/src/protocol.rs:403–485` (`begin_download`, `update_download`, `complete_download`, `abort_download` — без вызывов), `:1761` `browser_get_downloads`.
- **Сделать:** `AutomationCommand::GetDownloads`, `StartDownload {url}`, `CancelDownload {id}` + вариант ответа со списком (`id`, `url`, `file_name`, `state`, `received_bytes`, `total_bytes`, путь). MCP: `list_downloads {}`, `start_download {url}`, `cancel_download {id}`. BiDi: `browser.getDownloads` и события `browser.download*` — кормить от менеджера.
- **Готово, когда:** живая проба: `start_download` с адресом локального файла → `list_downloads` содержит элемент в состоянии `completed` с путём файла; повторный `start_download` + `cancel_download` — состояние `aborted`; `browser.getDownloads` больше не пуст после старта загрузки.
- **Зависит от:** MCP-TOTAL-S1b.

## MCP-TOTAL-S7d: `release_actions` и колесо мыши
- **Точка входа:** `crates/shell/src/app/automation_cmds.rs:258` (arm `PointerDown`), `:328` (`PointerUp`), `:382–398` (`Key`, `injected_modifiers`); обработчик настоящего `MouseWheel` — в `crates/shell/src/app/window_event/` (файл развести); `crates/shell/src/lumen/automation.rs:524` `scroll_by_delta`; BiDi — `crates/bidi-server/src/protocol.rs:2632` `input_release_actions` (validate-then-ACK), `:2511–2513` (оговорка про wheel sources).
- **Сделать:** `AutomationCommand::ReleaseActions` — отпустить всё зажатое автоматизацией (кнопки, модификаторы `injected_modifiers`), дослав странице `pointerup`/`mouseup`; `AutomationCommand::Wheel {x, y, delta_x, delta_y, modifiers}` — настоящий `MouseWheel` в точке тем же обработчиком, что событие окна. MCP: `release_actions {}`, `wheel {x, y, delta_x, delta_y}`. BiDi: `input.releaseActions` и wheel-действия `input.performActions`.
- **Готово, когда:** живая проба: `pointer_down` без `pointer_up`, затем `release_actions` → страница получила `pointerup` и `mouseup`, `:active` снят; `wheel {x, y, delta_x: 0, delta_y: 120}` вызывает обработчик `wheel` со `deltaY` того же знака и меняет `window.scrollY`.
- **Зависит от:** MCP-TOTAL-S1b.

## MCP-TOTAL-S7e: `reload`, `go_back`, `go_forward`
- **Точка входа:** `crates/shell/src/app/automation_cmds.rs:20` (arm `Navigate`), `:44–90` (вкладки); обработчики тулбара «назад», «вперёд», «перезагрузка» — развести (начать с `crates/shell/src/chrome_action.rs`); в BiDi обработчиков `browsingContext.reload` и `browsingContext.traverseHistory` нет (таблица — `crates/bidi-server/src/protocol.rs:671–720`).
- **Сделать:** `AutomationCommand::Reload {ignore_cache: bool}` и `TraverseHistory {delta: i32}`; ответ — новый адрес и флаги «можно назад/вперёд». MCP: `reload {ignore_cache?}`, `go_back {}`, `go_forward {}`. BiDi: `browsingContext.reload`, `browsingContext.traverseHistory`.
- **Готово, когда:** живая проба: `navigate` на A, затем на B → `go_back` → `eval("location.href")` даёт A; `go_forward` → B; на первой записи истории `go_back` отвечает ошибкой, а не «успехом»; `reload` перезагружает страницу (счётчик в `sessionStorage` вырос).
- **Зависит от:** MCP-TOTAL-S1b.

## MCP-TOTAL-S7f: `eval` с `await_promise`
- **Точка входа:** `crates/bidi-server/src/protocol.rs:1220` `script_evaluate` (читает `awaitPromise`), `:1346` `eval_await_promise`; MCP — `crates/mcp/src/server.rs:356–372` (схема `eval`), `:804` (arm); `crates/driver/src/live_session.rs:361` `eval`, `:371` `eval_with_timeout`.
- **Сделать:** приём «выражение-промис → результат» опустить с уровня BiDi на уровень драйвера (общий метод), чтобы им пользовались оба фронта; в MCP `eval` добавить необязательный `await_promise: bool`. Сегодня промис из MCP `eval` возвращается как `{}` (наблюдение 2026-11-26).
- **Готово, когда:** MCP `eval {code: "Promise.resolve(41 + 1)", await_promise: true}` возвращает `42`; с отклонённым промисом — ошибку с текстом причины; без флага — как раньше; BiDi `script.evaluate {awaitPromise: true}` работает как до среза (тесты `crates/bidi-server/src/protocol/tests.rs` зелёные).
- **Зависит от:** MCP-TOTAL-S1a.

## MCP-TOTAL-S2a: честные `Err` для молчаливых заглушек x-поверхности
- **Точка входа:** `crates/driver/src/live_session.rs:194–257` (`layout_snapshot_scoped` — `:194`, `computed_style` — `:226`, `computed_style_snapshot` — `:231`, `layout_box_by_selector` — `:236`, `all_layout_boxes_by_selector` — `:241`, `explain_element` — `:250`, `explain_page` — `:256`), `:389` `query_scoped`; текст соседних `Err`-заглушек — `:204`.
- **Сделать:** заменить `Ok(пусто)` на `Err` того же вида, что у `screenshot_scoped` (`:204`). Методы `computed_style_snapshot`, `layout_box_by_selector`, `all_layout_boxes_by_selector` в MCP не выставлены — остаются с честным `Err` и в дальнейшем.
- **Готово, когда:** тест `lumen-driver`: `LiveWindowSession` на каждом из восьми методов возвращает `Err` (как построить сессию без окна — смотреть существующие тесты заглушек); на живом окне `x-computed-style {selector: "#nope"}` отвечает ошибкой «не реализовано», а не `null`.
- **Гейт:** `cargo clippy -p lumen-driver --all-targets -- -D warnings`; `cargo test -p lumen-driver`.
- **Зависит от:** — (не зависит от S1; ставится в очереди перед S2b).

## MCP-TOTAL-S2b: display list на живом окне
- **Точка входа:** заглушки `crates/driver/src/live_session.rs:212` `display_list_scoped`, `:221` `display_list`; headless-образцы — `crates/driver/src/session.rs:1027`, `crates/driver/src/scope.rs`; в шелле — dump-путь `--dump-display-list` (`crates/shell/src/dump_mode.rs:917` `run_dump`) и образец переноса рендера в automation — `render_current_page_to_png` (`crates/shell/src/lumen/automation.rs:558`, DEVX-14); потребители MCP уже есть: `resource://x-display-list` (`crates/mcp/src/server.rs:626`), `x-scope-display-list` (`:877`).
- **Сделать:** варианты `DisplayList` и `DisplayListScoped {selector}` + arm'ы + ответы; текст — тот же формат, что у `--dump-display-list`.
- **Готово, когда:** на живом окне `resource://x-display-list` возвращает непустой текст с командами страницы; `x-scope-display-list {selector}` — только поддерево (короче целого); headless-ответы не изменились (`cargo test -p lumen-driver`).
- **Зависит от:** MCP-TOTAL-S2a.

## MCP-TOTAL-S2c: scoped layout и query на живом окне
- **Точка входа:** `crates/driver/src/live_session.rs:194` `layout_snapshot_scoped`, `:389` `query_scoped`; образцы — `crates/driver/src/scope.rs:34` `collect_boxes`, headless — `session.rs:1027`; потребители — `x-scope-layout` (`crates/mcp/src/server.rs:857`), `x-scope-query` (`:891`).
- **Сделать:** варианты `LayoutSnapshotScoped {selector}`, `QueryScoped {root_selector, selector}` + arm'ы.
- **Готово, когда:** `x-scope-layout {selector: "#box"}` возвращает только боксы поддерева (меньше, чем `resource://layout`); `x-scope-query {root_selector: "#box", selector: "a"}` находит ссылки только внутри `#box`.
- **Зависит от:** MCP-TOTAL-S2a.

## MCP-TOTAL-S2d: computed style и explain на живом окне
- **Точка входа:** `crates/driver/src/live_session.rs:226` `computed_style`, `:250` `explain_element`, `:256` `explain_page`; образцы — `crates/driver/src/explain.rs`, headless — `session.rs:1027`; потребители — `x-computed-style` (`crates/mcp/src/server.rs:925`), `x-explain-element` (`:835`), `x-explain-page` (`:844`).
- **Сделать:** варианты `ComputedStyle {selector}`, `ExplainElement {selector}`, `ExplainPage` + arm'ы.
- **Готово, когда:** `x-computed-style {selector: "#box"}` возвращает свойства (не `null`) и совпадает по `color` с `eval("getComputedStyle(document.querySelector('#box')).color")`; `x-explain-element` — непустая цепочка; `x-explain-page` — ненулевые счётчики боксов.
- **Зависит от:** MCP-TOTAL-S2a.

## MCP-TOTAL-S2e: scoped screenshot, relayout и eval на живом окне
- **Точка входа:** `crates/driver/src/live_session.rs:203` `screenshot_scoped`, `:395` `relayout_scoped`, `:401` `eval_scoped`; образец рендера — `render_current_page_to_png` (`crates/shell/src/lumen/automation.rs:558`); потребители — `x-scope-screenshot` (`crates/mcp/src/server.rs:867`), `x-scope-relayout` (`:901`), `x-scope-eval` (`:915`).
- **Сделать:** варианты `ScreenshotScoped {selector}`, `RelayoutScoped {selector}`, `EvalScoped {selector, code}` + arm'ы.
- **Готово, когда:** `x-scope-screenshot {selector}` возвращает PNG с размерами элемента (сверить с `eval("JSON.stringify(document.querySelector(sel).getBoundingClientRect())")`); `x-scope-eval` выполняет код с областью видимости поддерева; `x-scope-relayout` пересчитывает поддерево (изменение ширины после правки стиля видно в `resource://layout`). Известное ограничение остаётся: per-frame оверлеи (каретка, орфография) в снимок не попадают (`docs/engine-gaps.md:18`).
- **Зависит от:** MCP-TOTAL-S2a.

## MCP-TOTAL-S4a: viewport в рантайме (эмуляция)
- **Точка входа:** `crates/shell/src/cli_args.rs:249` `extract_viewport_override` (как применяется к окну — развести), `crates/shell/src/window_metrics.rs`; BiDi — `crates/bidi-server/src/protocol.rs:2096` `bc_set_viewport` (хранит размер только в состоянии протокола, комментарий `:2095`). Оконные вызовы winit — на потоке event loop; образец отложенного применения — слив fullscreen-запросов в `crates/shell/src/app/about_to_wait.rs:1503`.
- **Сделать:** `AutomationCommand::SetViewport {width, height}` — эмуляция viewport страницы (решение №8), не resize OS-окна; отсутствие размера — сброс. MCP: `set_viewport {width?, height?}`. BiDi: `browsingContext.setViewport` — в живое окно.
- **Первый шаг:** развести, что делает `--viewport`; если он меняет размер OS-окна, а не эмулирует, — сначала предложить пользователю механизм эмуляции и ждать ответа.
- **Готово, когда:** `set_viewport {width: 375, height: 667}` → `eval("[innerWidth, innerHeight]")` = `[375, 667]`, медиазапрос `max-width: 400px` срабатывает, раскладка пересчитана; размер OS-окна не изменился (проверить `get_window_rect` из S4b или вручную); без аргументов — возврат к размеру окна.
- **Зависит от:** MCP-TOTAL-S1b.

## MCP-TOTAL-S4b: fullscreen, фокус и rect окна
- **Точка входа:** `crates/shell/src/app/about_to_wait.rs:1503` (`w.set_fullscreen(target)`), `crates/shell/src/lumen/keyboard.rs:263`, `crates/shell/src/lumen/media_controls.rs:593`, `:609`, `crates/js/src/v8_runtime/install/platform.rs:1433` (`take_fullscreen_requests`); winit 0.30 (`Cargo.toml:111`); BiDi — `crates/bidi-server/src/protocol.rs:1042` `bc_activate`.
- **Сделать:** `AutomationCommand::SetFullscreen {enabled}`, `FocusWindow`, `GetWindowRect` + вариант ответа `{x, y, width, height, maximized, minimized}`. Вызовы окна — из потока event loop. MCP: `set_fullscreen {enabled}`, `focus_window {}`, `get_window_rect {}`. BiDi: `browsingContext.activate` — фокус окна.
- **Готово, когда:** живая проба: `set_fullscreen {enabled: true}` → `get_window_rect` показывает размер экрана, `eval("[innerWidth, innerHeight]")` вырос, страница продолжала работать; `{enabled: false}` возвращает прежний размер; `focus_window` ставит окно Lumen на передний план (проверить руками или `GetForegroundWindow`).
- **Зависит от:** MCP-TOTAL-S1b.

## MCP-TOTAL-S3a: drag-and-drop через pointer-примитивы
- **Точка входа:** `crates/shell/src/app/automation_cmds.rs:258` (arm `PointerDown`), `:328` (`PointerUp`), `:236` (`PointerMove` — уже идёт через настоящий `on_cursor_moved`); образцы — `crates/shell/src/app/window_event/mouse_input.rs:319–331` (создание `DndState` над draggable-элементом), `:649–666` (drop + dragend), `crates/shell/src/app/window_event/cursor_moved.rs:102–149` (активация после OS-порога, события drag); `crates/shell/src/lumen/pointer.rs:216–228` `js_drag_event`.
- **Сделать:** в arm `PointerDown` — создавать `DndState` тем же кодом, что `mouse_input.rs:319–331`; в arm `PointerUp` — завершать drop + dragend, как `mouse_input.rs:649–666`. Середина цепочки (`PointerMove`) уже идёт через `on_cursor_moved`.
- **Готово, когда:** живая проба: `pointer_down` на draggable-элементе, `pointer_move` за порог на drop-зону, `pointer_up` → на источнике сработали `dragstart` и `dragend`, на приёмнике — `dragenter`, `dragover`, `drop` (лог событий собран в массив через `eval`).
- **Зависит от:** MCP-TOTAL-S1b.

## MCP-TOTAL-S3b: BiDi — drag-жесты
- **Точка входа:** `crates/bidi-server/src/protocol.rs:2511–2513` (оговорка «NOT modeled: … drag gestures, or `pointerUp`-gated release semantics»), `:2514` `replay_input_actions`.
- **Сделать:** после S3a последовательность `pointerDown` → `pointerMove` → `pointerUp` в `input.performActions` должна давать drag без дополнительного кода — проверить, закрепить тестом, обновить комментарий и `docs/automation.md` (§WebDriver BiDi, `:126`). Действия `pause` — исполнить, если тривиально; иначе записать оговорку точнее.
- **Готово, когда:** тест BiDi (или живая проба через `--bidi-port`) с drag-последовательностью даёт те же события, что MCP-проба из S3a.
- **Зависит от:** MCP-TOTAL-S3a.

## MCP-TOTAL-S3c: модель `DataTransfer` (разведка, дизайн, нарезка)
- **Точка входа:** `crates/shell/src/lumen/pointer.rs:216–228` (`js_drag_event` передаёт пустой `'{}'`), `crates/js/src/shim/web_api_shim_mid.js:907` `_lumen_dispatch_drag_event(nid, type, x, y, data_json)`; модели данных в `crates/js` нет.
- **Сделать:** эта сессия — разведка и дизайн, без реализации: как хранить данные перетаскивания (`dataTransfer.setData/getData/types`, чтение на `drop`), как шеллу передать данные, установленные в `dragstart`, в последующие события. Дизайн согласовать с пользователем, затем нарезать реализацию на срезы (строки — в конец `ROADMAP.md`, указатели — в конец `STATUS-P6.md`, разделы — в конец этого файла) и перейти к следующей задаче.
- **Готово, когда:** дизайн согласован с пользователем, срезы заведены.
- **Зависит от:** MCP-TOTAL-S3a.

## MCP-TOTAL-S5a: `resource://network` — `blocked`, `failed`, `duration_ms`
- **Точка входа:** `crates/driver/src/types.rs:162–171` (`NetworkEntry` на проводе: `url`, `method`, `status`, `size_bytes`), `crates/shell/src/devtools/network_panel.rs:39–63` (поля `blocked`, `failed`, `reason`, `duration_ms`), arm — `crates/shell/src/app/automation_cmds.rs:575` `NetworkLog`, ресурс — `crates/mcp/src/server.rs:617`.
- **Сделать:** добавить в `lumen_driver::NetworkEntry` поля `blocked`, `failed`, `reason`, `duration_ms` (serde-по умолчанию — обратная совместимость), заполнить их в arm.
- **Готово, когда:** заблокированный правилом запрос в `resource://network` имеет `blocked: true` и непустой `reason`; обычный — `duration_ms > 0`; `cargo test -p lumen-driver` и `cargo test -p lumen-shell` зелёные.
- **Гейт:** `cargo clippy -p lumen-driver -p lumen-shell -p lumen-mcp --all-targets -- -D warnings`; `scripts/scoped-test.sh`.
- **Зависит от:** — (не зависит от S1 и S2).

## MCP-TOTAL-S5b: `resource://x-mem-report`
- **Точка входа:** `crates/shell/src/app/about_to_wait.rs:82` (`LUMEN_MEM_REPORT`), `crates/shell/src/lumen/state.rs:407–415` (`ENGINE_SUMMARY`), `crates/js/src/v8_runtime/runtime.rs:2188` `debug_heap_stats` (вызов — `crates/shell/src/persistent_js.rs:1278`).
- **Сделать:** первый шаг — свести, что печатает `LUMEN_MEM_REPORT`; команда `MemReport` с ответом-носителем тех же чисел; ресурс `resource://x-mem-report` с `"experimental": true`; тест ресурсов — с 6 на 7 (`crates/mcp/src/server.rs:1243`).
- **Готово, когда:** `resource://x-mem-report` на живом окне возвращает непустой отчёт, числа того же порядка, что в журнале под `LUMEN_MEM_REPORT=1`; headless — честная ошибка «только для живого окна».
- **Зависит от:** MCP-TOTAL-S2b (паттерн переноса и вариант-носитель ответа).

## MCP-TOTAL-S5c: `resource://x-frame-stats`
- **Точка входа:** `crates/engine/paint/src/lib.rs:86–93` `frame_log_level` (`LUMEN_FRAME_LOG`), потребители в шелле — `crates/shell/src/page_load.rs:1333`, `crates/shell/src/relayout.rs:544`, `:1043`, `crates/shell/src/bench_frames.rs:55`.
- **Сделать:** первый шаг — свести, что считают покадровые диагностики; ресурс `resource://x-frame-stats` (`experimental: true`): время paint-фазы и размер display list за последние N кадров (N — параметр ресурса).
- **Готово, когда:** ресурс возвращает записи за последние кадры после перерисовки страницы; числа того же порядка, что строки `[frame] …` под `LUMEN_FRAME_LOG=1`.
- **Зависит от:** MCP-TOTAL-S5b (тот же паттерн).

## MCP-TOTAL-S5d: `resource://x-animations`
- **Точка входа:** `crates/shell/src/animation_scheduler.rs:177` `AnimationScheduler` (событие тика — `:87`).
- **Сделать:** ресурс `resource://x-animations` (`experimental: true`): список отслеживаемых анимаций (узел, свойство, состояние) на момент запроса.
- **Готово, когда:** на странице с CSS-переходом и запущенным `element.animate(...)` ресурс перечисляет обе анимации во время их выполнения и пуст после завершения.
- **Зависит от:** MCP-TOTAL-S5b.

## MCP-TOTAL-S5e: `resource://x-font-stats`
- **Точка входа:** атлас глифов `GlyphAtlas` в `lumen-paint`; контракты кэшей — `crates/core/src/ext.rs:3746–3811`.
- **Сделать:** первый шаг — развести, какие счётчики уже есть у атласа (заполнение, число глифов, промахи, вытеснения); ресурс `resource://x-font-stats` (`experimental: true`). Если счётчиков нет — добавить минимальные в `lumen-paint`.
- **Готово, когда:** ресурс возвращает счётчики атласа; после загрузки страницы с новым шрифтом числа растут.
- **Зависит от:** MCP-TOTAL-S5b.

## MCP-TOTAL-S5f: `resource://x-stylesheets`
- **Точка входа:** `crates/shell/src/stylesheets.rs`, `document.styleSheets` в `crates/js/src/shim/web_api_shim_mid.js`; смежный открытый баг — BUG-1554 (`BUGS.md:206`).
- **Сделать:** ресурс `resource://x-stylesheets` (`experimental: true`): список листов страницы — адрес, `disabled`, `media`, число правил.
- **Готово, когда:** на странице с `<link>` и `<style>` ресурс перечисляет оба листа с ожидаемыми полями; по ходу отметить в BUG-1554, воспроизводится ли он на живом окне.
- **Зависит от:** MCP-TOTAL-S5b.

## MCP-TOTAL-S7g: сеть — тела ответов и override (разведка и нарезка)
- **Точка входа:** `crates/network/src/intercept.rs`; BiDi — `crates/bidi-server/src/protocol.rs:1499` `network_get_response_body` (хранилище `response_bodies` заполняет только `:614` `record_response_body` — вызывов нет), `:1996` `resolve_intercepted_request` (читает только `request`, поля override молча отбрасываются), `:1953` `network_continue_response` (override отвергает — `:1957–1963`).
- **Сделать:** эта сессия — разведка и нарезка, без реализации: (g1) тело ответа — `network.getResponseBody` и MCP `get_response_body {request_id}`; (g2) override запроса — `url`, `method`, `headers`, `body`; (g3) override ответа — `status`, `headers`. Срезы дописать в конец `ROADMAP.md` и `STATUS-P6.md`, разделы — в конец этого файла; спорные решения согласовать с пользователем.
- **Готово, когда:** срезы заведены с точками входа в `intercept.rs`.
- **Зависит от:** MCP-TOTAL-S1d.

## MCP-TOTAL-S7h: печать живого документа в PDF
- **Точка входа:** `crates/shell/src/lumen/printing.rs:45` `handle_print_request`, `:93` `handle_print_confirm` (рендер подтверждённого задания в PDF; кодировка — в dump-пути, `crates/shell/src/dump_mode.rs`), `crates/shell/src/cli_args.rs:47` `extract_print_to_pdf`.
- **Сделать:** `AutomationCommand::PrintToPdf {path}` — рендер тем же кодом, что `handle_print_confirm`, без диалога. MCP: `print_to_pdf {path, overwrite?}` — существующий файл не перезаписывать молча.
- **Готово, когда:** живая проба: `print_to_pdf {path}` на текущей странице создаёт файл, начинающийся с `%PDF`, ненулевого размера; страница продолжает работать; существующий файл без `overwrite` — ошибка.
- **Зависит от:** MCP-TOTAL-S1b.

## HTTP-AUTH-LIVE: HTTP auth в живом окне (разведка, дизайн, нарезка)
- **Точка входа:** `crates/network/src/auth.rs` (Basic, Digest, retry 401), `crates/core/src/ext.rs:352` `HttpCredentialProvider` (имплементации — `StaticCredentialProvider`, `crates/network/src/auth.rs:663`, и тестовая, `crates/network/src/lib.rs:10551`; в `crates/shell` реализаций и вызовов нет); в `crates/shell` и `crates/js` код 401 встречается только в антибот-эвристике `crates/shell/src/gate_detect.rs:244`; образец диалога в хроме — `crates/shell/src/lumen/dialogs.rs`.
- **Сделать:** эта сессия — разведка и дизайн, без реализации: что происходит с ответом 401 в живом окне; провайдер credentials в шелле (запрос к пользователю, хранение на сессию); фаза `authRequired` перехвата (`network.continueWithAuth` и MCP `continue_with_auth` — после). Дизайн согласовать с пользователем, нарезать реализацию на срезы (строки — в конец `ROADMAP.md`, указатели — в конец `STATUS-P6.md`, разделы — в конец этого файла).
- **Готово, когда:** дизайн согласован с пользователем, срезы заведены.
- **Зависит от:** —

## MCP-TOTAL-S6: настоящий OS-ввод (не в очереди)
Ждёт отдельного решения пользователя (решение №3 от 2026-11-26). Ориентиры на будущее: в `crates/` вызовов `SendInput` нет, единственная реализация — внешняя (`scripts/scroll_smoothness_run.py:65`); по воспоминаниям сеанса 2026-11-25 браузер игнорирует `PostMessage`, работает только настоящий ввод при окне Lumen на переднем плане. User activation мотивом не является — automation-клик её уже даёт.
