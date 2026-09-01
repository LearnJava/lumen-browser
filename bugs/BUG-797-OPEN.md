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
