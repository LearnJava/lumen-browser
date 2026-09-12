# BUG-883 — `window.open()` (и клик по `<a target=_blank>`) убивает документ-вызыватель: ни один его таймер больше не срабатывает

**Статус:** OPEN (ДОРАБОТКА → [GAP-NAVCTX](../ROADMAP.md))
**Тип:** нереализованная функциональность, не дефект реализованного кода — ведётся как задача `GAP-NAVCTX` в [ROADMAP.md](../ROADMAP.md), P3 как баг не берёт. Переклассифицировано 2026-09-02 ре-триажем пула WPT-RUN-5/6: срезы заводили багом всё подряд, потому что правила заведения ([docs/probe-method.md §8](../docs/probe-method.md)) тогда ещё не было. Файл сохраняет номер и путь — на него ссылаются CLAUDE.md, STATUS-файлы и python-тулинг, а запись наблюдений остаётся полезной там, где лежит.
**Заведён:** 2026-08-23 (WPT-RUN-6, срез 28 — живой замер, вариант `win-open-freeze`)
**Область:** shell (`crates/shell/src/main.rs` — дренаж popup-запросов `_lumen_window_open`, `open_new_tab()`), js (`crates/js/src/dom.rs:12190-12220` — заглушка, которую `window.open` возвращает)
**Владелец:** P1/P3. Заведён P2 в ходе WPT-задачи, здесь не чинится.

## Симптом

После `window.open(url)` документ, который её вызвал, перестаёт исполняться
целиком: ни `setInterval`, ни `setTimeout`, поставленные **до** вызова, больше
не срабатывают, `visibilitychange` не приходит, `document.visibilityState`
остаётся `"visible"` — страница не «уходит в фон», она просто больше никогда
не получает управления. Открываемый документ при этом **загружается** (запрос
виден на сервере пробы), исполняет свои скрипты и логирует в тот же stderr.

Это отдельный барьер, стоящий **раньше** [BUG-797](BUG-797-OPEN.md) (заглушка
`window.open` без реального `opener`/`postMessage`): даже если бы канал был,
вызывающая сторона уже мертва и ответ услышать некому.

Тот же эффект даёт клик по `<a target="_blank">` (вариант
`win-anchor-target`): ноль тиков у страницы-инициатора, документ по ссылке
загружен.

## Прямое измерение

`tests/wpt/verify_window_history_jsurl_gaps.py --variant win-open-freeze`
(2026-08-23, dev-release, Linux, `main` = `0dc60692d`). Страница бьётся
таймером каждые 400 мс, на 2-й секунде вызывает `open()`, и держит два
одиночных таймера на 3,5 с и 6 с:

```
beat 1 vis=visible hidden=false
beat 2 … beat 3 … beat 4 … beat 5
opening at beat 5
opened w=object beat=5
child-ran search=?from=freeze opener=null parent-is-self=true name=undefined
[server saw: GET /vwjh-child.html?from=freeze]
```

Дальше — тишина: ни `beat 6`, ни `post-open-timer` (3,5 с), ни `late-timer`
(6 с) за оставшиеся 8 секунд. Контроль — вариант `control` того же прогона
даёт 14 тиков за 8 с, вариант `unload-nav` показывает, что обычная навигация
`location.href = …` ведёт себя так же (документ заменяется), то есть
`open()` обрабатывается как навигация **текущего** окна, а не как создание
вспомогательного контекста.

Смежные измерения того же прогона (вариант `win-open`/`win-open-detail`):
возвращаемая заглушка — обычный объект (`typeof w === "object"`,
`w === window` → `false`, `w.name` = переданное имя, `w.location.href` =
`about:blank` для `open()` без аргументов, `w.document` — `undefined`), а у
самого окна `window.closed` и `window.name` — `undefined`
([BUG-887](BUG-887-OPEN.md)). В открытом документе `window.opener === null` и
`window.parent === window`.

## Цена по WPT

Семь id остатка WPT-RUN-5, у которых ожидание стоит сразу за `open()`:

`html/browsers/browsing-the-web/navigating-across-documents/cross-origin-top-navigation-with-user-activation.window.html`,
`…-with-user-activation-in-parent.window.html`,
`…-without-user-activation.window.html`,
`…-without-user-activation-nested.window.html`,
`…/same-origin-top-navigation-without-user-activation.window.html`,
`html/browsers/windows/noreferrer-null-opener.html`,
`html/browsers/browsing-the-web/unloading-documents/prompt-and-unload-script-closeable.html`
(последний — вместе с [BUG-887](BUG-887-OPEN.md)).

Нижняя граница: всё семейство `RemoteContext`/`common/dispatcher` строится на
`open()`, и по BUG-797 оно уже числится за отдельным механизмом.

## Что дальше

HTML LS §7.2.2 «window open steps» создаёт **новый** browsing context;
исходный документ остаётся полностью живым и активным. Чинить вместе с
BUG-797: пока `open()` реализована как навигация текущего окна, ни канал, ни
`opener` смысла не имеют.

## Срез 2 (GAP-NAVCTX, 2026-09-12) — `<a target=_blank>` теперь открывает вкладку; opener получает `visibilitychange`

Три независимых куска той же причины закрыты частично:

* **`<a target="_blank">` клик по СТРАНИЦЕ** (`crates/shell/src/lumen/click.rs`)
  раньше вообще не создавал вкладку — `target` читался только для поиска
  именованного фрейма, а `_blank` (не совпадающий ни с одним фреймом) тихо
  проваливался в обычный `navigate_to(target)` **того же документа**: строго
  хуже `window.open()`, у которого вкладка хотя бы появлялась. Теперь `_blank`
  проверяется явно, до поиска именованного фрейма (тот же порядок, что уже
  использует `link_destination` для фреймовых ссылок), и уходит в
  `self.open_new_tab(); self.navigate_to(...)`.
* **`<a target="_blank">` клик из под-документа `<iframe>`**
  (`crates/shell/src/lumen/frame_links.rs`, `LinkTarget::NewWindow`) был
  заглушкой (`eprintln!` + `true`, без навигации вообще). Теперь резолвит
  `href` базой РЕБЁНКА (`nav_base.resolve_str`) и вызывает тот же
  `open_new_tab()`/`navigate_to()`.
* **Опенер молчал при уходе в фон.** `Lumen::open_new_tab()` парковал JS-хэндл
  опенера в `bg_tabs`, не сообщая ему об этом никак — в отличие от
  пользовательского переключения вкладки (`switch_tab`), которое перед парковкой
  зовёт `pause_event_loop()` (эта функция гонит `_lumen_apply_visibility(true)`
  синхронным `eval_js` — диспатчит `visibilitychange`, не зависит от очереди
  таймеров). Теперь `open_new_tab()` делает то же самое, так что и
  `window.open()`, и оба варианта клика по `target=_blank` больше не оставляют
  опенер без единого сигнала о том, что он ушёл в фон.

**Живая проверка** (`tests/wpt/verify_window_history_jsurl_gaps.py`, новый
вариант `win-open-visibility`, dev-release, `--mcp-live-port`): страница вешает
слушатель `visibilitychange` (не таймер) и зовёт `open()`; лог —
`opening, opened vis=visible hidden=false, vis-changed vis=hidden hidden=true,
child-ran …` — событие приходит СРАЗУ, синхронно с попап-дренажом, до того как
опенер получил бы следующий тик, будь у него таймер.

**Не в этом срезе (осознанная граница, не регрессия):**
- **Таймеры опенера всё ещё не тикают.** `pause_event_loop()` — только
  уведомление; физически двигок качает ровно один `js_ctx` за раз
  (`crates/shell/src/lumen/page_snapshot.rs`, документированный инвариант «Exactly
  one page is live at a time»). `win-open-freeze`-вариант того же скрипта
  подтверждает: `setInterval`/`setTimeout`-цепочка опенера останавливается на
  той же секунде, что и раньше. Спека этого не требует буквально останавливать
  таймеры фона (только троттлить), но заменить однопоточную качалку на
  параллельную (хотя бы для только что открывшего вкладку опенера) — отдельная,
  много большая архитектурная работа, не влезающая в этот срез.
- **BUG-797 (заглушка `opener`/`postMessage`) не тронута** — открытый документ
  по-прежнему получает `opener === null`.
- **Именованные (не `_blank`, не совпавшие ни с одним живым фреймом) `target`
  на ссылке СТРАНИЦЫ** — `click.rs`'s `named_frame` по-прежнему смотрит только
  на `<iframe>` того же документа, а не на другие открытые вкладки; несовпавшее
  имя падает в прежний `navigate_to` того же документа (уже существовавшее,
  более узкое ограничение, не расширено и не сужено этим срезом).
- **`window.close()`/`window.closed`/`window.name`** — отдельный баг
  ([BUG-887](BUG-887-OPEN.md)), не тронут.

Гейт: `cargo clippy -p lumen-shell --all-targets --features v8 -- -D warnings`
чисто; `scripts/scoped-test.sh` — единственный красный
(`cpu_snapshots_match_references`) тем же байт-в-байт списком, что и в срезе 1
GAP-NAVCTX (BUG-884) — предсуществующий дрейф, не от этой правки.
