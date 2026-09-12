# BUG-887 — `window.close()` — no-op, а `window.closed` и `window.name` не существуют (`undefined`)

**Статус:** OPEN (ДОРАБОТКА → [GAP-NAVCTX](../ROADMAP.md))
**Тип:** нереализованная функциональность, не дефект реализованного кода — ведётся как задача `GAP-NAVCTX` в [ROADMAP.md](../ROADMAP.md), P3 как баг не берёт. Переклассифицировано 2026-09-02 ре-триажем пула WPT-RUN-5/6: срезы заводили багом всё подряд, потому что правила заведения ([docs/probe-method.md §8](../docs/probe-method.md)) тогда ещё не было. Файл сохраняет номер и путь — на него ссылаются CLAUDE.md, STATUS-файлы и python-тулинг, а запись наблюдений остаётся полезной там, где лежит.
**Заведён:** 2026-08-23 (WPT-RUN-6, срез 28 — живой замер, варианты `win-close`/`win-open-detail`)
**Область:** js (`crates/js/src/dom.rs` — глобальный объект `window`; заглушка `window.open` на `:12210` определяет `closed`/`name` только у возвращаемого объекта, но не у самого окна)
**Владелец:** P1/P3. Заведён P2 в ходе WPT-задачи, здесь не чинится.

## Симптом

Три атрибута окна, которые спека требует всегда:

* `window.closed` — `undefined` (спека: `false`, пока контекст жив).
* `window.name` — `undefined` (спека: пустая строка). Присваивание
  «прилипает» (`window.name = "x"` читается обратно), но начальное значение
  не строка, поэтому `window.name === ""` — ложь, а любой тест на
  именованное таргетирование (`open(url, name)`) не находит контекст.
* `window.close()` — функция есть, вызов не бросает и не делает ничего:
  документ остаётся живым (15 тиков после вызова), `window.closed` как было
  `undefined`, ни `beforeunload`, ни `unload`, ни `pagehide` не приходят.

`pagehide` при этом **работает** на обычной навигации (вариант `unload-nav`
того же прогона), то есть дефект не в диспатче события, а в том, что
`close()` не запускает шаги выгрузки вообще. Отсутствие
`beforeunload`/`unload` — [BUG-834](BUG-834-FIXED.md).

## Прямое измерение

`tests/wpt/verify_window_history_jsurl_gaps.py --variant win-close --variant
win-open-detail --variant unload-nav` (2026-08-23, dev-release, Linux,
`main` = `0dc60692d`):

```
win-close        ticks=15  before-close closed=undefined hasclose=function
                           close-returned closed=undefined
                           after-close closed=undefined
win-open-detail  noargs type=object is-self=false opener=null closed=undefined
                           name=undefined hasfocus=function
                 name-set now="vwjh-named"
unload-nav       navigating-away
                 nav-pagehide
                 child-ran search=?from=unload …
```

Ни `beforeunload`, ни `unload` не напечатаны ни в одном из вариантов;
`nav-pagehide` — напечатан.

## Цена по WPT

Один id остатка WPT-RUN-5 целиком за этим багом:
`html/browsers/browsing-the-web/unloading-documents/prompt-and-unload-script-closeable.html`
(ждёт `beforeunload`, затем `unload`, оба вызванные `window.close()`; вторым
барьером там стоит [BUG-883](BUG-883-OPEN.md) — тест исполняется как
top-level, а не как открытый скриптом контекст).

`window.closed` читают ещё и все тесты семейства `RemoteContext`
(`t.add_cleanup(() => w.close())`), но там раньше срабатывает
[BUG-797](BUG-797-OPEN.md).

## Что дальше

HTML LS §7.2.2: `closed` — readonly-геттер, `name` — строковый атрибут с
дефолтом `""`, `close()` для script-closeable контекста запускает «prompt to
unload» и затем «unload». Минимум, снимающий id: определить `closed`/`name`
как свойства окна и провести `close()` через тот же путь выгрузки, что уже
диспатчит `pagehide` при навигации.

## Срез 3 (GAP-NAVCTX, 2026-09-12, `p1-gap-navctx-srez3`)

Сделан ровно минимум из раздела выше, без архитектурной части (реального
закрытия вкладки/browsing context — этот движок держит один JS-контекст на
страницу, и после `close()` скрипт продолжает исполняться):

* `window.name` — теперь `''` по умолчанию (`crates/js/src/shim/web_api_shim_tail_mc.js`,
  рядом с `window.open`/`window.close`); присваивание и раньше «прилипало»,
  дефект был только в отсутствующем дефолте.
* `window.closed` — сперва сделан readonly-геттером (`Object.defineProperty`,
  тот же паттерн, что `isSecureContext` в этом же файле), но это ломало
  `lumen-js`'s собственный тестовый набор: `PutValue` на аксессоре без
  сеттера — тихий no-op (не строгий режим), а два юнит-теста
  (`v8_details_dialog_popover::details_name_exclusivity_closes_other`,
  `v8_whatwg_streams::writable_stream_close_resolves`) заводят на верхнем
  уровне скрипта `var closed = …` как обычное локальное имя — после того как
  `window` становится `globalThis`, это тот же самый глобальный `closed`, и
  присваивание перестаёт долетать. Оставлено обычным присваиваемым
  свойством (`window.closed = false`/`= true`), как `opener`/`name` рядом —
  не строгая unforgeability по спеке, но WPT-id этого бага её не проверяет.
* `window.close()` — идемпотентно (не срабатывает повторно) зовёт
  `_lumen_fire_beforeunload()` затем `_lumen_unload_document(false)` — те же
  две функции, что шлёт из Rust `persistent_js.rs` при обычной навигации
  (BUG-834), — и выставляет `window.closed = true`.

Живой замер `verify_window_history_jsurl_gaps.py --variant win-close`
(dev-release): `before-close closed=false hasclose=function` →
`beforeunload, pagehide, unload` → `close-returned closed=true` →
`after-close closed=true` (было: `closed=undefined` до и после вызова, ни
одно из трёх событий не печаталось). Попутно `win-open-detail` подтверждает
дефолт `name=""` на объекте-заглушке `open()` не тронут (он уже был
самостоятельным путём) — регрессии нет ни в одном из 24 вариантов пробы.
`cargo test -p lumen-js --features v8-backend` — 3585 passed, 0 failed
(первая попытка с readonly-геттером дала 2 failed — та самая коллизия,
починено до коммита).

Не входило: `prompt-and-unload-script-closeable.html` (единственный id этого
бага в остатке WPT-RUN-6/28) по-прежнему не снят — тест открывает себя как
top-level страницу, а не как контекст, открытый скриптом, и вторым барьером
там стоит [BUG-883](BUG-883-OPEN.md) (таймеры опенера всё ещё не тикают в
фоне); `closed` для тестов `RemoteContext` по-прежнему упирается в
[BUG-797](BUG-797-OPEN.md) (нет канала `postMessage` к настоящему опенеру).
Фактическое закрытие вкладки/browsing context из скрипта — отдельная
архитектурная работа, не в этом срезе.
