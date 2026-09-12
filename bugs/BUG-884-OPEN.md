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

## Срез 3 (GAP-NAVCTX срез 7, 2026-09-12/13, `p1-gap-navctx-srez7`) — переприсваивание `.src` уже загруженного фрейма

Закрыт хвост, который срез 2 назвал «настоящей архитектурной работой»: доводы
были неверными, не подтвердились чтением кода. `run_frame_navigation`
(`frames.rs`, background thread) уже несёт `FrameNavPrep::parent_js` —
`Arc<dyn PersistentJs>` клонируется в `prepare_frame_navigation` ДО спавна
потока и передаётся в `spawn_frame` тем же параметром, что и для первичной
вставки (срез 6 сам его туда прокинул, просто не воспользовался для
`dest: Some(..)`). А сама реализация (`V8JsRuntime::run`,
`crates/js/src/v8_runtime/runtime.rs:1094`) тоннелирует ЛЮБОЙ вызов через
`SyncSender<V8Command>` на выделенный JS-поток — она рассчитана на вызов с
произвольного потока по конструкции (тот же приём, каким ADR-014 держит
QuickJS-совместимость), а не только с UI-потока. Правка — расширение матча в
`spawn_frame`: `dest: Some((href, _))` теперь тоже проверяется на
`javascript:`-схему ДО `fetch_iframe_source`, тем же `eval_iframe_javascript_url`
(контекст РОДИТЕЛЯ — тот же компромисс среза 6, не расширен и не сужен).

**Не-строковое завершение при навигации — отдельная ветка, не общая со
срезом 6.** Для первичной вставки «не навигация» и «остаться на пустом
`about:blank`» совпадают (фрейм и так на пустом документе). Для навигации
`.src` уже ЗАГРУЖЕННОГО фрейма — нет: свалить туда же значило бы стереть
живой документ ребёнка (свой JS-контекст, возможно, вложенные фреймы) пустым
`about:blank`, хотя спека (HTML LS §7.4.5) явно требует не навигировать
вовсе. `spawn_frame` теперь возвращает пустой `Vec` в этом случае — тот же
сигнал «навигация отклонена», который `apply_frame_navigation` уже понимает
для generation-гонки (см. его doc-comment); история не трогается.

**Живая проверка** (`verify_window_history_jsurl_gaps.py --variant
jsurl-iframe`, dev-release, Windows, `--mcp-live-port`): `jsurl-iframe-final
ran=2` — код при `fr.src = fr.src + ';'` теперь исполняется второй раз
(`jsurl-iframe-ran 2` печатается), что и требовал ожидаемый маркер
(`iframe_javascript_url_initial_insertion.html` строит ассерт именно на этом
счётчике). До правки — A/B на срезе 6 (`git stash`) — тот же вариант
показывал `jsurl-iframe-final ran=1`, подтверждая, что улучшение от этой
правки, а не от чего-то ещё в дереве. Соседние варианты (`jsurl-nav`,
`frame-parser`, `frame-navigate`, `frame-late-src`) — без изменений
относительно уже задокументированного поведения; `frame-navigate` теряет
второй `frame-load` (навигация ДОЧЕРНЕГО документа через
`fr.contentWindow.location.href=`, не через `.src` родителя) — подтверждено
A/B тем же `git stash`, идентичный вывод уже на срезе 6, известный
самостоятельный хвост, не в этой карточке.

**Не в этом срезе:** сама навигация фрейма через ЕГО СОБСТВЕННЫЙ
`location.href=`/клик (не через `.src` родителя) — отдельный путь
(`frame-navigate` выше); для глубины ≥ 1 `javascript:` во `<iframe src>`
по-прежнему читает `parent` РОДИТЕЛЯ, а не ребёнка (хвост среза 6, не тронут).

Гейт: `cargo build -p lumen-shell --profile dev-release` чисто;
`cargo clippy -p lumen-shell --all-targets -- -D warnings` и
`cargo clippy --workspace --all-targets -- -D warnings` — оба чисто;
`cargo test --bin lumen scripts_and_frames` (84/84) зелёный. `scoped-test.sh`
не запустился до конца — известная поломка гейта (BUG-805, `lumen-network`
виснет), не от этой правки: отдельный `cargo test -p lumen-network --lib`
(2207/2207 за 4.01 с) прошёл в рамках гейта до BUG-805 тем же приёмом, что и
срез 5. Полный WPT-прогон категории (`run_report.py --all --root
html/semantics/embedded-content/the-iframe-element`) не запускался до конца:
`--help` самого скрипта прямо предупреждает, что `--all` по невыверенным
категориям (iframes — в их числе) — режим обзора, не гейт; живого проба
(`verify_window_history_jsurl_gaps.py`, byte-точно предсказывающего этот же
счётчик) достаточно для минимума карточки.

## Срез 4 (GAP-NAVCTX срез 8, 2026-09-13, `p1-gap-navctx-srez8`) — `location.href=` фрейма через `contentWindow`, минуя `.src` родителя

Закрыт последний нетронутый хвост варианта `frame-navigate`: не `javascript:`,
а обычная навигация вида `fr.contentWindow.location.href = "relative.html"`
(частый паттерн, которым один документ переключает URL СОСЕДНЕГО фрейма, не
трогая его собственный атрибут `src`). До правки `w.location` фасада
(`winFacade(bid)`, `crates/js/src/frame_bridge.rs`) был геттером, отдававшим
одноразовый объектный литерал `{href, toString}` — запись `.href = url` молча
падала на этом выброшенном объекте, ничего не долетая ни до какой навигации,
и второй `frame-load` не наступал никогда.

**Правка** — геттер `location` теперь строит объект с настоящим
акцессором на `href` (плюс `assign`/`replace`, плюс сеттер на сам `location`
для формы `w.location = url`), и все четыре формы записи форвардятся в новый
хелпер `navigateFrameHost(bid, hostNid, url)`. Хелпер переиспользует тот же
натив записи атрибута, каким уже пишет `iframe.src=` (`_lumen_f_set_attr` для
bid-предка, `_lumen_make_element(hostNid).setAttribute('src', …)` для обычного
bid) — навигация приходит как `delta.changed` в `frame_dynamic.rs::poll_dynamic_frames`
на следующем тике, тем же путём, что уже прошли срезы 6/7, без отдельного
пути навигации и без нового натива навигации.

**Компромисс:** относительный URL резолвится против БАЗЫ ДОКУМЕНТА-ВЛАДЕЛЬЦА
хоста (как у обычного `src=`), а не против базы документа, из которого читают
`contentWindow.location` — спека резолвит навигацию против базы навигирующего
документа. Расходится только когда родитель и потомок лежат в разных базах;
не проверено этим срезом (в живом пробе оба документа лежат в одной
директории, поэтому база совпадает у обоих).

**Юнит-тест** (`frame_bridge.rs::parent_facade_location_setter_writes_host_src_attribute`,
без прод-обвязки poll_dynamic_frames) — все четыре формы (`.href=`,
`.assign()`, `.replace()`, `location=`) кладут `src` хоста в дереве родителя.
**Живая проверка** (`verify_window_history_jsurl_gaps.py --variant
frame-navigate`, dev-release, Windows, `--mcp-live-port`): было
`frame-nav-final loads=1` и один `GET` на сервере пробы; стало
`frame-load #1`+`frame-load #2` и оба `GET` (`?from=first`, `?from=second`).
A/B тем же `git stash` подтвердил, что улучшение — от этой правки, а не от
чего-то ещё в дереве.

Гейт: `cargo build -p lumen-shell --profile dev-release` чисто;
`cargo clippy -p lumen-js --all-targets --features v8-backend -- -D warnings`,
`cargo clippy -p lumen-shell --all-targets -- -D warnings` и
`cargo clippy --workspace --all-targets -- -D warnings` — все чисто;
`cargo test -p lumen-js --features v8-backend frame_bridge` (51/51) и
`cargo test --bin lumen scripts_and_frames` (84/84) зелёные.

**Не в этом срезе:** для глубины ≥ 1 `javascript:` во `<iframe src>`
по-прежнему читает `parent` РОДИТЕЛЯ, а не ребёнка (хвост среза 6); базовый
URL навигации через `contentWindow.location` резолвится против документа
хоста, а не документа-читателя (компромисс выше). Оба хвоста GAP-NAVCTX
(BUG-883 таймеры опенера в фоне, BUG-797 заглушка `opener: null` для обычной
навигации) не тронуты.

## Срез 5 (GAP-NAVCTX срез 9, 2026-09-13, `p1-gap-navctx-srez9`) — база резолва `contentWindow.location=` теперь читателя, а не хоста

Закрыт компромисс среза 4 (`GAP-NAVCTX срез 8`): `navigateFrameHost(bid,
hostNid, url)` (`crates/js/src/frame_bridge.rs`) писал `url` дословно в
атрибут `src` хоста, и относительный URL резолвился против базы
документа-ВЛАДЕЛЬЦА хоста (как обычный `src=`) — расходится со спекой,
которая резолвит навигацию против базы НАВИГИРУЮЩЕГО (читающего фасад)
документа.

**Правка** — `navigateFrameHost` теперь резолвит `url` через
`_url_resolve(url, _lumen_document_base_url())` ДО записи в `src` хоста. Это
безопасно ровно потому, что вся функция (как и весь `winFacade`/бридж-скрипт)
выполняется в JS-реалме ЧИТАТЕЛЯ — `_lumen_document_base_url()` в этом
контексте это база читателя, а не хоста. Получив уже абсолютный URL, хост
его не переразрешает, поэтому база хоста из среза 8 больше ни на что не
влияет. `_url_resolve`/`_lumen_document_base_url` отсутствуют в минимальных
тестовых изолятах без прод-шима — код на этот случай не трогает поведение
(пишет `url` как раньше), так что старый юнит-тест среза 8 (с абсолютными
URL) не меняет исход.

**Юнит-тест** (`frame_bridge.rs::parent_facade_location_setter_resolves_relative_url_against_reader_base`)
ставит стаб `_lumen_document_base_url`, изображающий базу читателя, заведомо
отличную от базы хоста (`parent.example`), и проверяет, что относительный
`vwjh-child.html`-подобный URL попадает в `src` хоста уже резолвленным
против базы ЧИТАТЕЛЯ, а не хоста.

**Живая проверка** (`verify_window_history_jsurl_gaps.py --variant
frame-navigate`, dev-release) — регрессии нет: оба `frame-load` и оба `GET`
на сервере пробы по-прежнему на месте (в этой фикстуре родитель и потомок
лежат в одной директории, поэтому базы совпадают и старое/новое поведение
неразличимо этим пробом; расхождение проявляется только при разных базах,
что и покрывает юнит-тест выше).

Гейт: `cargo build -p lumen-shell --profile dev-release` чисто;
`cargo clippy -p lumen-js --all-targets --features v8-backend -- -D warnings`
чисто; `cargo test -p lumen-js --features v8-backend --lib frame_bridge`
(52/52) зелёный — но обнаружен ПРЕДСУЩЕСТВУЮЩИЙ (не от этой правки) флейк:
`inaccessible_bridge_mutation_does_not_mark_dirty` иногда падает независимо
от содержимого правки (воспроизведено и на `main` до этого среза, ~1 раз из
5 прогонов) — глобальный `HashSet<usize>` в `frame_dom_dirty()` ключуется
сырым адресом `Arc::as_ptr`, и при переиспользовании освобождённого адреса
новым `Arc` соседний тест изредка видит чужой недренированный флаг. Не
исправлено в этом срезе (вне бюджета, отдельная архитектурная тема —
реестру нужен генерационный счётчик или другой ключ вместо адреса).

**Не в этом срезе:** для глубины ≥ 1 `javascript:` во `<iframe src>`
по-прежнему читает `parent` родителя, а не ребёнка (хвост среза 6); оба
хвоста GAP-NAVCTX (BUG-883, BUG-797) не тронуты.
