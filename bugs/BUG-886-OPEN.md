# BUG-886 — обход истории не диспатчит `popstate`, если запись создана `pushState(state, "")` без третьего аргумента; с URL — диспатчит

**Статус:** OPEN
**Заведён:** 2026-08-23 (WPT-RUN-6, срез 28 — живой замер, варианты `hist-popstate`/`hist-popstate-late`/`hist-pushstate-url`)
**Область:** shell (`crates/shell/src/main.rs:20618-20646` — ветка same-document в `nav_back`: `popstate` летит только если у записи есть `same_doc_state_json`), js (`crates/js/src/dom.rs:6921` — `pushState`)
**Владелец:** P1/P3. Заведён P2 в ходе WPT-задачи, здесь не чинится.

## Симптом

`history.back()`/`history.go(-1)` по записи, созданной самой частой в WPT
формой `history.pushState(state, "")` (без URL), **не диспатчит `popstate`
никогда** — ни через `window.onpopstate`, ни через `addEventListener`, ни
через контент-атрибут `<body onpopstate>`. При этом `history.state`
обновляется синхронно, то есть обход состоялся: событие теряется, а не
навигация.

Стоит передать третий аргумент — `pushState(state, "", "?a")` — и `popstate`
приходит. Записи, созданные `location.hash = "#1"`, тоже обходятся с
`popstate`.

Это уточняет [BUG-829](BUG-829-OPEN.md), где записано «доставка `popstate`
обеими формами регистрации работает»: то измерение делалось с URL-аргументом,
и для формы без URL оно неверно.

## Прямое измерение

`tests/wpt/verify_window_history_jsurl_gaps.py --variant hist-popstate-late
--variant hist-pushstate-url` (2026-08-23, dev-release, Linux,
`main` = `0dc60692d`). Слушатели поставлены до `pushState`, окно ожидания — 3 с:

```
hist-popstate-late   late-pushed len=2
                     late-back-called state=null
                     late-t1 state=null            (+1,0 с)
                     late-t2 state=null len=2      (+3,0 с)
hist-pushstate-url   url-pushed href=?a search= len=2
                     url-back-called
                     url-popstate state=null search= hash=
                     url-t1 state=null href=?a
```

`late-popstate` не напечатан ни разу; `url-popstate` — напечатан. Третий
вариант (`hist-popstate`, две записи `pushState(s, "")` подряд и два шага
назад) не дал ни одного из трёх маркеров при том, что `after-go
state={"x":1}` показывает состоявшийся переход.

Смежное, того же прогона: `history.scrollRestoration` — `undefined`;
`pushState(s, "", "#frag")` кладёт `"#frag"` в `location.href` целиком, а
`location.hash` остаётся пустым (это [BUG-829](BUG-829-OPEN.md), из-за чего
последующий обход такой записи тоже молчит); `location.reload()` и
`history.go(0)` работают полностью — три поколения документа и **три**
запроса на сервере.

## Цена по WPT

Три id остатка WPT-RUN-5, ожидающие ровно этого события:

`html/browsers/history/the-history-interface/005.html`
(`<body onpopstate>` + `history.go(-1)`),
`html/browsers/history/the-history-interface/back-pushstate-back-history-state.html`,
`html/browsers/browsing-the-web/overlapping-navigations-and-traversals/anchor-fragment-history-back-on-click.html`
(здесь `popstate` приходит один раз вместо двух — запись `#3` создана кликом,
запись, на которую возвращаются, — фрагментная).

## Что дальше

HTML LS §7.4.6: `pushState` без URL сохраняет текущий URL документа, но
запись создаётся полноценная, и «traverse the history» обязан диспатчить
`popstate` для любой same-document записи. Смотреть надо, что попадает в
`same_doc_state_json` записи, которую `pushState` кладёт в `nav_back`, когда
URL не менялся: ветка same-document в `main.rs:20621` срабатывает только по
наличию этого поля, иначе уходит в полную перезагрузку документа.
