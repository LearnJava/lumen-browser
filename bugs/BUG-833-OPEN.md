# BUG-833 — клик по `<a href="#x">` перезагружает документ вместо фрагментной навигации: `hashchange` не приходит, страница стартует заново (на автокликающей странице — бесконечный цикл)

**Статус:** OPEN
**Заведён:** 2026-08-22 (WPT-RUN-6, срез 21 — найден живым замером, маркера намеренно нет)
**Область:** `crates/js/src/dom.rs:14634` (`_lumen_run_activation_behavior`, ветка `A`/`AREA` → `_lumen_navigate`), `crates/js/src/dom.rs:6330` (`_lumen_navigate_or_fragment` — правильный путь, который здесь не используется)
**Владелец:** P1/P3 (`lumen-js`). Заведён P2 в ходе WPT-задачи, здесь не чинится.

## Симптом

```html
<a id="a" href="#x">go</a>
<script>document.getElementById("a").click();</script>
```

Вместо перехода по фрагменту (обновить `location.hash`, поставить
`hashchange`, проскроллить к цели) браузер выполняет **полную навигацию**:
документ загружается заново, весь скрипт выполняется с нуля, `location`
приходит без фрагмента. Страница, которая кликает по такой ссылке из
скрипта, уходит в бесконечный цикл перезагрузок.

Программная фрагментная навигация тем же URL работает: `location.href = "#x"`
проходит через `_lumen_navigate_or_fragment` и ведёт себя правильно.

## Прямое измерение

`tests/wpt/verify_navigation_form_import_gaps.py --variant anchor-fragment-plain`
(2026-08-22, dev-release, Linux, коммит `762a0cad9`, `--seconds 6`):

| ожидалось | получено |
|---|---|
| `anchor-click`, `hashchange hash=#pnfi-3`, `after-click hash=#pnfi-3` | `anchor-click defaultPrevented=false` — и сразу новый парс документа |

Тиков — **0**: интервал в 500 мс ни разу не успевает сработать, потому что
каждые ~200 мс документ начинается заново. В stderr это видно как
повторяющийся блок `Получено 773 байт (streaming)` → `script-start search=`
→ `anchor-click` без единого сетевого запроса между ними.

Соседняя проба `anchor-fragment-click` (та же ссылка, но обработчик клика
вызывает `history.back()`) показывает вторую половину картины: `hashchange`
от двух *программных* присваиваний приходит, `popstate` тоже — то есть
механизм фрагментов исправен везде, кроме активации ссылки.

## Причина (локализована чтением кода)

```js
if (tag === 'A' || tag === 'AREA') {         // dom.rs:14634
    var href = el.href;
    if (href) _lumen_navigate(String(href), false);
    return;
}
```

`_lumen_navigate` — путь полной навигации. Функция-обёртка
`_lumen_navigate_or_fragment` (`dom.rs:6330`), которая сравнивает URL без
фрагмента с текущим и в случае совпадения делает same-document переход,
существует и используется для `location.href=`/`assign()`/`replace()`, но
активация ссылки её обходит. HTML LS §7.4.2 не различает эти два входа:
решение «фрагмент или загрузка» принимается по URL, а не по способу.

## Масштаб

Маркера в `timeout_audit.py` намеренно нет: из остатка WPT-RUN-5 на этой
форме стоит только
`html/browsers/browsing-the-web/overlapping-navigations-and-traversals/anchor-fragment-history-back-on-click.html`,
и у него ожидание всё равно упирается в обход истории (BUG-835), так что
честного правила «по исходнику видно, что тест висит именно из-за этого»
не выводится. Заводится по прямому замеру, как [BUG-825](BUG-825-OPEN.md) и
[BUG-829](BUG-829-OPEN.md).

Цена вне WPT существенно выше: якорная навигация по одностраничному
документу — базовый механизм оглавлений, «наверх», табов и anchor-роутинга.
Сегодня любой такой клик перезагружает страницу со всей потерей состояния,
а если ссылка кликается скриптом — вешает вкладку в цикл.

## Направление починки (не предписание)

Заменить в ветке `A`/`AREA` вызов `_lumen_navigate` на
`_lumen_navigate_or_fragment(href, false)` — вся нужная логика там уже есть.
Отдельным шагом (уже за пределами этого бага) — скролл к цели фрагмента,
`:target` и обновление истории.

## Как проверить фикс

1. `tests/wpt/.venv/bin/python tests/wpt/verify_navigation_form_import_gaps.py
   --variant anchor-fragment-plain` — ожидаются `hashchange hash=#pnfi-3` и
   `after-click hash=#pnfi-3`, тиков > 0.
2. WPT: `run_report.py --all --root html/browsers/browsing-the-web --recursive`.
