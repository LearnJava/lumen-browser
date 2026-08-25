# BUG-829 — `history.pushState`/`replaceState` кладут в `location` сырой аргумент: `location.href` становится относительной строкой, `search`/`pathname` протухают

**Статус:** FIXED 2026-08-25 (P1)
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
[BUG-825](BUG-825-FIXED.md) в срезе 19.

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

## Починка (P1, 2026-08-25)

Заявка описывала один дефект, а их оказалось **три**, каждый на своей границе.
Первый — тот, что в заголовке; два других вскрылись только при проверке
и без них «обход» так и остался бы сломанным.

### 1. Резолвинг URL (`crates/js/src/dom.rs`, шим)

`pushState`/`replaceState` теперь гоняют аргумент через новый
`_lumen_history_state_url`: `new URL(url, _lumen_document_base_url()).href` —
именно **базовый URL документа**, как требует HTML LS §7.4.6 шаг 3, а не
`location.href` (разница видна на странице с `<base href>`). При неудачном
разборе и при отказе `_lumen_history_can_rewrite_url` бросается
`SecurityError`, причём **до** любой записи в историю: отклонённый вызов не
должен оставлять следов, поэтому резолвинг стоит первой строкой метода, до
`_lumen_history_push`.

`_lumen_history_can_rewrite_url` — это спековское «can have its URL rewritten»:
отличие по схеме/логину/паролю/хосту/порту отвергается сразу; http(s)-документ
после этого свободен внутри своего origin (путь, запрос, фрагмент), а под любой
другой схемой (`file:`, `about:`, `data:`, `blob:`) отличаться может только
фрагмент. Спека называет `file:` отдельным шагом, но следующий её шаг всё равно
накрывает для него и запрос, так что оба шага свелись к одному сравнению.

Заодно изменилась трактовка пустой строки: `url` — nullable DOMString со
значением по умолчанию `null`, поэтому «URL не трогаем» означает только
**отсутствующий** (или явно `null`) аргумент, а `""` — обычная относительная
ссылка, резолвящаяся в базовый URL. Раньше обе формы схлопывались в `if
(target)` и вели себя одинаково.

### 2. Обход не восстанавливал URL (`crates/shell/src/main.rs`)

До первого `pushState` поле `display_url` пусто — URL документа лежит в
`source`, — поэтому запись, которую дренаж `pushState` кладёт в `nav_back`,
уходила туда **без URL вообще**. `fire_popstate` отдавал JS пустую строку, а
`_lumen_deliver_popstate` читает её как «URL не менять», так что возврат
восстанавливал состояние и оставлял `location` на пушнутом адресе. Теперь
дренаж падает на URL самого документа — тем же способом, что и
`current_display_url`.

### 3. Обход терял *состояние* (`fire_popstate`)

Самое дорогое из трёх и в заявке отражено только симптомом «`history.state`
равен `null`». `fire_popstate` собирал `_lumen_deliver_popstate({state_json},
'{url}')`, встраивая JSON **голым литералом** — по рассуждению «валидный JSON
и есть валидное JS-выражение». Но шим принимает первым аргументом JSON-**текст**
и зовёт на нём `JSON.parse`, так что до него доезжал объектный литерал,
`JSON.parse({n:1})` бросал, и обход отдавал `state: null` для *любого*
непустого состояния. Незамеченным это оставалось потому, что единственное
значение, переживающее путаницу без изменений, — сам `null`. Инструментация
показала это прямо: шелл держал `state=Some("{\"n\":1}")`, страница получала
`null`.

Починено сериализацией текста в JSON-строку (`serde_json::to_string`), то есть
в JS-строковый литерал с уже экранированными кавычками, слэшами и управляющими
символами. Сборка вызова вынесена в свободную функцию `popstate_eval_source`,
чтобы обе кодировки аргументов можно было проверить юнит-тестом без живого
рантайма — путаница между ними и была багом. Отдельно важен строковый state
(`pushState("hi")`): «голый» вариант доезжал как JS-строка `hi`, и `JSON.parse`
её отвергал, — поэтому «принимать оба вида аргумента» на стороне шима было бы
неоднозначно и от такого варианта пришлось отказаться.

### Замер

`verify_preload_script_audio_gaps.py --seconds 6` (dev-release, Windows),
три исторические пробы — все три совпали со своей колонкой `expected`:

| проба | было | стало |
|---|---|---|
| `history-pushstate-url` | `after-push href=?psag-push search=` | `after-push href=http://127.0.0.1:13057/.psag-history-pushstate-url.html?psag-push search=?psag-push state={"n":1}`; `after-relative href=http://127.0.0.1:13057/psag-relative.html` |
| `history-go-popstate` | `popstate state=null`, `after-go search=` | `popstate state={"n":1}`, `after-go search=?psag1 state={"n":1}` |
| `history-back` | `popstate-listener state=null, after-back search=` | то же (исходный документ и был без запроса — проба проверяет, что возврат не оставляет `?psagb`) |

Юнит-тесты: `lumen-js` — резолвинг запроса/относительного пути/фрагмента для
обеих функций, `SecurityError` на чужом origin и на другом порту (с проверкой,
что `history.length` и `location` не сдвинулись); `lumen-shell` —
`popstate_eval_source` на объектном, строковом и `null`-состоянии плюс
экранирование обоих аргументов. Восемь существующих тестов истории переведены
с `v8_runtime_with_dom` (документ без URL) на `v8_runtime_with_url`, а шесть
проверок, дословно фиксировавших дефект (`location.href === '/page2'`),
переписаны на резолвнутый URL.

### Что не входило

`history.pushState(state, "")` **без** третьего аргумента по-прежнему не даёт
`popstate` при обходе — это [BUG-886](BUG-886-OPEN.md), отдельный механизм
(запись без URL вообще не доезжает до стека шелла). Здесь он не тронут.
