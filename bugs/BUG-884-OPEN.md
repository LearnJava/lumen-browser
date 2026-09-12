# BUG-884 — `javascript:`-URL не исполняется нигде: ни в `<iframe src>`, ни по клику, ни через `location.href`, ни в `open()` — уходит в сеть как «unsupported scheme»

**Статус:** OPEN (ДОРАБОТКА → [GAP-NAVCTX](../ROADMAP.md))
**Тип:** нереализованная функциональность, не дефект реализованного кода — ведётся как задача `GAP-NAVCTX` в [ROADMAP.md](../ROADMAP.md), P3 как баг не берёт. Переклассифицировано 2026-09-02 ре-триажем пула WPT-RUN-5/6: срезы заводили багом всё подряд, потому что правила заведения ([docs/probe-method.md §8](../docs/probe-method.md)) тогда ещё не было. Файл сохраняет номер и путь — на него ссылаются CLAUDE.md, STATUS-файлы и python-тулинг, а запись наблюдений остаётся полезной там, где лежит.
**Заведён:** 2026-08-23 (WPT-RUN-6, срез 28 — живой замер, варианты `jsurl-iframe`/`jsurl-nav`)
**Область:** shell (`crates/shell/src/main.rs` — `resolve_js_navigation`, `load_frame_sub_documents`: `javascript:`/`data:` «отклоняются с логом»), js (`crates/js/src/dom.rs` — `_lumen_navigate_or_fragment`, `window.open`)
**Владелец:** P1/P3. Заведён P2 в ходе WPT-задачи, здесь не чинится.

## Симптом

URL со схемой `javascript:` нигде не воспринимается как код. Замерены все
четыре места, где WPT его ставит:

* **`<iframe src="javascript:…">`, написанный парсером** — код не исполняется
  (счётчик в родителе остаётся `0`), `load` на элементе не диспатчится вообще
  ни в одной форме регистрации. Присваивание `iframe.src = iframe.src + ";"`
  проходит без исключения и тоже ничего не запускает.
* **`<a href="javascript:…">` + `click()`** — клик отрабатывает
  (`jsurl-anchor-clicked` печатается), код не исполняется.
* **`location.href = "javascript:…"`** — присваивание не бросает, но
  `location.href` остаётся прежним, код не исполняется.
* **`open("javascript:…")`** — уходит в сетевой слой: в логе браузера
  `network error: unsupported scheme: javascript`. Заодно уносит документ
  ([BUG-883](BUG-883-OPEN.md)).

## Прямое измерение

`tests/wpt/verify_window_history_jsurl_gaps.py --variant jsurl-iframe
--variant jsurl-nav` (2026-08-23, dev-release, Linux, `main` = `0dc60692d`):

```
jsurl-iframe   ticks=15  jsurl-iframe-state ran=0 src=javascript:(function(){ parent cw=object
                         jsurl-iframe-reassigned
                         jsurl-iframe-final ran=0
jsurl-nav      ticks=0   jsurl-anchor-clicked
                         jsurl-location-assigned href=http://127.0.0.1:45661/.vwjh-jsurl-nav.h
                         jsurl-open-returned null=false
                         …network error: unsupported scheme: javascript
```

Ни `jsurl-iframe-ran`, ни `jsurl-iframe-load`, ни `jsurl-anchor-ran`, ни
`jsurl-location-ran`, ни `jsurl-open-ran` не напечатаны. Ловушка при чтении
лога: сам текст URL содержит подстроку маркера, поэтому строка сетевой ошибки
выглядит как сработавший маркер — сверять надо по счётчику `ran=`, а не по
наличию имени маркера в строке.

Контроль: парсерный `<iframe src="vwjh-child.html">` в том же прогоне
(вариант `frame-parser`) загружается, исполняет скрипт и диспатчит `load`,
то есть дефект именно в схеме, а не в элементе.

## Цена по WPT

Пять id остатка WPT-RUN-5:

`html/semantics/embedded-content/the-iframe-element/iframe_javascript_url_initial_insertion.html`,
`…/iframe_javascript_url_not_about_blank.html`,
`content-security-policy/navigation/to-javascript-url-frame-src.html`,
`xhr/open-url-javascript-window.htm`,
`xhr/open-url-javascript-window-2.htm`.

Последние два упираются в [BUG-885](BUG-885-FIXED.md) (фрейм создан скриптом)
на шаг раньше — но и с загруженным фреймом `javascript:parent.request()`
остался бы неисполненным.

## Что дальше

HTML LS §7.4.5 «javascript: URL special case»: навигация на `javascript:`
исполняет код в контексте *инициатора* и, если результат — строка,
заменяет документ. Минимум для перечисленных id — исполнять код и не
отправлять URL в сетевой слой. Порядок с [BUG-885](BUG-885-FIXED.md) любой:
дефекты независимы.

## Срез 1 (GAP-NAVCTX, 2026-09-12) — `location.href`/`.replace()`/`open()`/клик по `<a>`

Закрыты три из четырёх мест: `location.href=`/`location.assign`/`location.replace`
(`JsNavigateRequest::Push`/`Replace`, `about_to_wait.rs`), `window.open(...)`
(тот же файл, `take_window_open_requests`-цикл) и клик по `<a href="javascript:…">`
(`lumen/click.rs`, до проверки `target`/`is_navigable_href`). Новый общий путь:
`page_source::javascript_url_code(url)` вырезает код после схемы (регистронезависимо,
без percent-decode — редкая необходимость для `javascript:`, ревизия при первом
WPT-id, которому это будет нужно), `Lumen::eval_javascript_url` (`navigation.rs`)
гоняет его в ТЕКУЩЕМ JS-контексте через новый `PersistentJs::eval_js_completion`
(отличает JS-строку от прочего — старый `eval_js_value` терял это различие через
JSON-круговорот), и если результат — строка, документ заменяется
`PageSource::Static { html, url }` (URL исходного документа сохраняется) через
`navigate_replace` — само `javascript:` не создаёт новую запись истории, поэтому
`Push` (location.href=) и `Replace` (`.replace()`) сходятся на одном вызове.
Не-строковый результат (`undefined`, объект…) — молчаливый no-op, ничего не
заменяется, ничего не грузится.

**Упрощения этого среза** (не регрессии, осознанные границы):
- `PageSource::Static` не несёт origin/resource_base — замена document по
  `javascript:` не сохраняет исходный origin для последующих same-origin проверок
  и не резолвит относительные подресурсы к прежнему base URL. Для минимальных id
  (строковый литерал вместо документа) это не задевается.
- Клик по `<a href="javascript:…" target="_blank">` игнорирует `target` — код
  исполняется и подставляется в ТЕКУЩИЙ документ, а не в новый контекст;
  маршрутизация в другой browsing context не имеет смысла, пока опенер всё ещё
  гибнет при `window.open()` ([BUG-883](BUG-883-OPEN.md)).
- Тот же гейт (`is_navigable_href`) в остальных пяти местах, где он
  переиспользуется (клик внутри фрейма, hint-mode, форма `GET`,
  `winit_session.rs`), не тронут — там `javascript:`-ссылка по-прежнему тихо
  игнорируется, как раньше.
- `window.open("javascript:...")`: код исполняется в контексте ОПЕНЕРА (верно
  по спеке), результат подставляется в открытую вкладку как `PageSource::Static`
  с `url: "about:blank"` — но сама вкладка становится активной (BUG-883
  по-прежнему не чинен в этом срезе), так что опенер всё ещё замирает сразу
  после.

**Не в этом срезе:** `<iframe src="javascript:…">` (парсерная и через
переприсваивание `.src`) — `fetch_iframe_source` (`frames.rs`) вызывается вне
UI-потока с текущей архитектурой (нет доступа к `Arc<dyn PersistentJs>`
родителя на этом шаге), а исполнение в контексте РЕБЁНКА требует, чтобы у
фрейма уже был установлен JS-контекст на пустом `about:blank` документе до
получения источника — separate slice. Оставшиеся два WPT id из «Цены по WPT»
(`xhr/open-url-javascript-window*`) тоже не закрыты этим срезом — оба упираются
в iframe-путь.

Живая проверка (MCP `--mcp-live-port`, `LUMEN_NO_ENGINE_THREAD=1`, реальное
окно): `location.href = 'javascript:"loc-replaced"'` и клик по
`<a href="javascript:'anchor-replaced'">` оба заменили документ строкой
completion без обращения к сети (скриншот подтверждён визуально); `window.open`
не проверен живьём в этом срезе (требует переключения вкладки через MCP,
оставлено — код идентичен уже проверенному пути).

Гейт: `cargo test --bin lumen javascript_url_code` (2 новых теста),
`cargo clippy -p lumen-shell --all-targets -- -D warnings` чисто;
`scoped-test.sh` — единственный красный (`cpu_snapshots_match_references`)
подтверждён A/B на `main` тем же байт-в-байт списком несовпадений,
предсуществующий дрейф (см. [BUG-1008](BUG-1008-OPEN.md)/BUG-517 хвост),
не от этой правки.

## Срез 2 (GAP-NAVCTX срез 6, 2026-09-12) — `<iframe src="javascript:…">`, первичная вставка

Закрыта первая из двух половин, оставленных срезом 1 «не в этом срезе»:
`spawn_frame` (`frames.rs`) теперь распознаёт `javascript:` в `src` разметки
ДО вызова `fetch_iframe_source` (который по-прежнему отказывает такой схеме —
его собственный юнит-тест не тронут) и исполняет код через новый
`eval_iframe_javascript_url`, строковое завершение становится HTML фрейма
(`FrameSource::Inline`, адрес — `about:blank`, той же логикой, что и обычный
`about:blank`-фрейм — `javascript:` не заводит запись истории). Покрыты ОБА
call-сайта, идущих через `dest: None` (`spawn_frame`'s собственный параметр):
парсерная вставка (`load_frame_sub_documents`) и `document.createElement
('iframe')` с сразу выставленным `src` (`frame_dynamic_load.rs::
run_new_frame_load`, тот же `dest: None`) — оба используют один и тот же
путь в `spawn_frame`.

**Компромисс вместо архитектуры среза 1:** карточка предполагала, что нужен
собственный JS-контекст РЕБЁНКА на пустом `about:blank` до получения
источника. Реализовано и ОТБРОШЕНО в этом же срезе — второй живой V8-изолят
на том же потоке (пробный контекст + основной чуть позже) вешает движок
насмерть без единого паник-сообщения: страница переставала печатать тики
`setInterval` сразу после `PROBE script-start`, тем же почерком, что уже
описан в `frame_dynamic.rs::dispatch_pending_frame_loads` doc-comment для
другого случая создания V8-изолята не в том месте. Вместо этого код
исполняется в контексте РОДИТЕЛЯ (`parent_js`, который `spawn_frame` и так
получает параметром) — тот же компромисс, каким `BUG-883` срез 2 уже
пожертвовал для `window.open()`. Для фрейма глубины 0 разницы не видно
(`window.parent === window` у верхней страницы), поэтому WPT-тест
(`parent.javascriptUrlRan++`) её не ловит; для глубины ≥ 1 код читает `parent`
РОДИТЕЛЯ, а не самого фрейма (на один уровень не то) — известный хвост.

**Не в этом срезе (важнее предыдущего пункта):** переприсваивание `.src` уже
ЗАГРУЖЕННОГО фрейма (`iframe.src = iframe.src + ';'` из
`iframe_javascript_url_initial_insertion.html`, весь
`iframe_javascript_url_not_about_blank.html`) идёт СОВСЕМ другим путём —
`frame_dynamic.rs::poll_dynamic_frames` кладёt такую смену в `delta.changed`,
а не `delta.new`, и это уходит в `navigate_frame_to` →
`replace_frame_document` → `run_frame_navigation`, который САМ выполняется
на фоновом потоке (`std::thread::spawn`, см. doc-comment
`replace_frame_document`) — то самое «`fetch_iframe_source` вызывается вне
UI-потока, нет доступа к `Arc<dyn PersistentJs>` родителя на этом шаге»,
которое карточка среза 1 назвала причиной отложить ВЕСЬ `<iframe>`-путь. Это
и есть настоящая архитектурная работа: нужен тот же приём, что топ-уровневый
`Lumen::eval_javascript_url` уже использует для `location.href=` —
`route_query_js` до потока, которому принадлежит JS-контекст, а не прямой
вызов с фонового потока. Живой проб (`verify_window_history_jsurl_gaps.py
--variant jsurl-iframe`) это подтверждает: `jsurl-iframe-ran 1` и
`jsurl-iframe-load` печатаются (первичная вставка исполнилась), но
`jsurl-iframe-final ran=1` вместо ожидаемого `ran=2` — переприсваивание `.src`
код не повторило. Ни один WPT id из «Цены по WPT» этим срезом целиком не
закрыт (`iframe_javascript_url_initial_insertion.html` — `single_test`,
падает на второй половине).

Живая проверка: `tests/wpt/verify_window_history_jsurl_gaps.py --variant
jsurl-iframe --variant jsurl-nav --variant frame-parser --variant
frame-navigate --variant frame-late-src` (dev-release, Windows) — `jsurl-iframe`
показывает описанное выше улучшение, остальные четыре — без изменений
относительно уже задокументированного поведения (регрессии нет).

Гейт: `cargo clippy -p lumen-shell --all-targets -- -D warnings` чисто;
`scoped-test.sh` — два красных, оба предсуществующие и не от этой правки:
`cpu_snapshots_match_references` (тот же дрейф, что и срез 1) и
`lumen-network::fetch_range_200_fallback_when_server_ignores_range` (внешний
флейк, не задет диффом — правка не трогает `lumen-network`).
