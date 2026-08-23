# BUG-884 — `javascript:`-URL не исполняется нигде: ни в `<iframe src>`, ни по клику, ни через `location.href`, ни в `open()` — уходит в сеть как «unsupported scheme»

**Статус:** OPEN
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

Последние два упираются в [BUG-885](BUG-885-OPEN.md) (фрейм создан скриптом)
на шаг раньше — но и с загруженным фреймом `javascript:parent.request()`
остался бы неисполненным.

## Что дальше

HTML LS §7.4.5 «javascript: URL special case»: навигация на `javascript:`
исполняет код в контексте *инициатора* и, если результат — строка,
заменяет документ. Минимум для перечисленных id — исполнять код и не
отправлять URL в сетевой слой. Порядок с [BUG-885](BUG-885-OPEN.md) любой:
дефекты независимы.
