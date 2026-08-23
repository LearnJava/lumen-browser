# BUG-887 — `window.close()` — no-op, а `window.closed` и `window.name` не существуют (`undefined`)

**Статус:** OPEN
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
`beforeunload`/`unload` — [BUG-834](BUG-834-OPEN.md).

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
