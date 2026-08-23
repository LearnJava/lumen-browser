# BUG-829 — `history.pushState`/`replaceState` кладут в `location` сырой аргумент: `location.href` становится относительной строкой, `search`/`pathname` протухают

**Статус:** OPEN
**Заведён:** 2026-08-22 (WPT-RUN-6, срез 20 — найден живым замером, маркера намеренно нет)
**Область:** `crates/js/src/dom.rs:6921` (`history.pushState` → `_lumen_location_update(target)`), `crates/js/src/dom.rs:6930` (`replaceState`, то же самое), `crates/js/src/dom.rs:6231` (`_lumen_location_update` — `_lumen_parse_url(url)` от нерезолвнутой строки), `crates/js/src/dom.rs:11438` (комментарий в шиме, который это поведение уже фиксирует)
**Владелец:** P1/P3 (`lumen-js`). Заведён P2 в ходе WPT-задачи, здесь не чинится.

## Симптом

После `pushState` объект `Location` перестаёт быть абсолютным URL:

```js
// страница http://127.0.0.1:38595/probe.html
history.pushState({n: 1}, "", "?psag-push");
location.href       // → "?psag-push"      (должно: "http://127.0.0.1:38595/probe.html?psag-push")
location.search     // → ""                (должно: "?psag-push")
history.state       // → {n: 1}            — единственное, что верно
history.pushState({n: 3}, "", "psag-relative.html");
location.href       // → "psag-relative.html"
```

`replaceState` ведёт себя так же. Обратный обход тоже не восстанавливает
URL: после `history.go(-1)` событие `popstate` приходит (со `state: null`),
но `location.search` не меняется, а `history.state` равен `null`.

## Прямое измерение

`tests/wpt/verify_preload_script_audio_gaps.py` (2026-08-22, коммит
`79f7df91a`, `--seconds 5`, все пробы живы — по 9 тиков):

| проба | получено |
|---|---|
| `history-pushstate-url` | `before href=http://127.0.0.1:38595/… search= length=1`; `after-push href=?psag-push search= state={"n":1} length=2`; `after-replace href=?psag-replace state={"n":2}`; `after-relative href=psag-relative.html` |
| `history-go-popstate` | `pushed length=3 search=`; `popstate state=null`; `popstate-listener`; `after-go search= state=null` |
| `history-back` | `popstate-listener state=null`; `after-back search=` |

То есть работают: счётчик `history.length`, `history.state` сразу после
`pushState`, доставка `popstate` обоими способами регистрации. Не работают:
резолвинг URL и восстановление записи при обходе.

## Причина (локализована чтением кода)

`history.pushState` (`dom.rs:6921`) передаёт аргумент как есть:

```js
pushState: function(state, title, url) {
    var target = String(url !== undefined && url !== null ? url : '');
    …
    if (target) { _lumen_location_update(target); _lumen_history_push_url(target, new_state_json); }
}
```

а `_lumen_location_update` (`dom.rs:6231`) просто разбирает эту строку:
`_lumen_loc_parts = _lumen_parse_url(url)`. Резолвинга относительно базового
URL документа нет ни там, ни там, поэтому в `Location` оказывается ровно то,
что написал автор страницы. HTML LS §7.4.6 требует обратного: URL сначала
парсится **относительно базового URL документа**, при неудаче бросается
`SecurityError`, и только результат становится URL записи истории.

Поведение уже описано в самом шиме: комментарий у `isSecureContext`
(`dom.rs:11438`) объясняет, почему флаг снимается один раз при установке —
«a same-document `history.pushState(s, '', '/x')` stores that raw relative
string in `_lumen_loc_parts`, which would flip the flag to false on an https
page». То есть о дефекте знали и обошли его в одном месте, вместо того чтобы
починить источник; из-за этого сегодня `isSecureContext` — единственный
потребитель `_lumen_loc_parts`, которого дефект не задевает.

## Масштаб

Маркера в `timeout_audit.py` намеренно нет, и это осознанное решение среза:
остаточных id `html/browsers/history` — 11, но у большинства из них ожидание
стоит на `<iframe>` (BUG-480) или на обходе, а не на чтении `location`, так
что честного правила «по исходнику видно, что тест висит именно из-за
этого» вывести не удалось. Заводится по прямому замеру, как
[BUG-825](BUG-825-OPEN.md) в срезе 19.

Цена вне WPT прямее, чем внутри: любой SPA-роутер (а это подавляющее
большинство современных сайтов) после первой же навигации получает
`location.href` вида `/products/42` и `location.search === ''`. Всё, что
дальше строит абсолютные ссылки, читает query-параметры или сравнивает
origin, работает по мусору — и это на *успешном* пути, без единой ошибки в
консоли.

## Направление починки (не предписание)

В `pushState`/`replaceState` резолвить `url` относительно базового URL
документа (`new URL(url, _lumen_loc_href).href`) до вызова
`_lumen_location_update`, и на исключении бросать `SecurityError`, как велит
спека. Отдельным шагом — обход: `_lumen_deliver_popstate` уже умеет принимать
URL записи, значит шелл должен отдавать его вместе с состоянием, а не
оставлять `location` нетронутым.

## Как проверить фикс

1. `tests/wpt/.venv/bin/python tests/wpt/verify_preload_script_audio_gaps.py
   --variant history-pushstate-url --variant history-go-popstate` —
   `after-push href=http://…/probe.html?psag-push search=?psag-push`,
   после `go(-1)` `search` возвращается к предыдущему.
2. WPT: `run_report.py --all --root html/browsers/history --recursive`.

## Уточнение (WPT-RUN-6, срез 28, 2026-08-23)

Строка выше «работают при этом `history.length`, `history.state` сразу после
`pushState` и доставка `popstate` обеими формами регистрации» верна только
для `pushState` **с URL-аргументом** — так это и было измерено (`?x`). Для
самой частой в WPT формы `history.pushState(state, "")` без третьего
аргумента `popstate` при обходе не приходит вообще ни в одной форме
регистрации, хотя `history.state` обновляется синхронно: отдельный дефект,
заведён как [BUG-886](BUG-886-OPEN.md).

Там же перепроверено и подтверждено ядро этого бага:
`pushState({u:1}, "", "?a")` даёт `location.href` = `?a` при пустом
`location.search`, а `pushState({h:1}, "", "#frag")` — `href` = `#frag` при
пустом `location.hash`.
