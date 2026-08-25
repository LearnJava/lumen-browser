# BUG-837 — activation behaviour ищется на самом кликнутом узле, а не на ближайшем активируемом предке: клик по `<span>` внутри `<label>` (или внутри `<a>`/`<button>`) не делает ничего

**Статус:** FIXED 2026-08-25 (P1)
**Заведён:** 2026-08-22 (WPT-RUN-6, срез 21 — найден живым замером, маркера намеренно нет)
**Область:** `crates/js/src/dom.rs:14679` (`HTMLElement.prototype.click` → `_lumen_run_activation_behavior(nid, this)` для самого узла), `crates/js/src/dom.rs:4030` (тот же вызов на пути `dispatchEvent`), `crates/js/src/dom.rs:14617` (`_lumen_run_activation_behavior` — таблица по тегу узла)
**Владелец:** P1/P3 (`lumen-js`). Заведён P2 в ходе WPT-задачи, здесь не чинится.

## Симптом

```html
<label for="cb"><span id="t">peas?</span></label>
<input type="checkbox" id="cb">
<script>document.getElementById("t").click();</script>   // ничего не происходит
```

Событие `click` доставляется и всплывает как положено, но активация не
выполняется: чекбокс не переключается, `click`/`change` на нём не приходят,
фокус не переносится. Клик по самому `<label>` работает полностью.

## Прямое измерение

`tests/wpt/verify_navigation_form_import_gaps.py` (2026-08-22, dev-release,
Linux, коммит `762a0cad9`, `--seconds 6`; обе страницы живы — по 11 тиков):

| проба | получено |
|---|---|
| `label-click-activates` (клик по `<span>` внутри label) | `clicked-span checked=false active=` — ни `cb-click`, ни `change` |
| `label-click-direct` (клик по самому `<label>`) | `control=pnfi-cb labels=1`, `cb-click`, `change checked=true`, `clicked-label checked=true` |

Вторая проба — контроль: ассоциация `label.control`/`input.labels` и ветка
`LABEL` в таблице активации исправны. Разница ровно в том, на какой узел
пришёлся клик.

## Причина (локализована чтением кода)

```js
var notCancelled = _lumen_dispatch_rich(nid, ev);
if (notCancelled) _lumen_run_activation_behavior(nid, this);   // dom.rs:14679
```

и то же самое в `dispatchEvent` (`dom.rs:4030`). `nid` — узел, по которому
кликнули; `_lumen_run_activation_behavior` (`:14617`) сразу берёт его тег и
идёт по таблице `INPUT`/`BUTTON`/`A`/`SUMMARY`/`LABEL`. Для `SPAN` ветки
нет — функция молча возвращается.

DOM Standard §2.9 требует иного: перед диспетчеризацией вычисляется
**activation target** — ближайший предок в пути события, у которого есть
activation behaviour, — и по завершении диспетчеризации активируется он, а
не target. Отсюда же вытекает, что клик по `<span>` внутри `<a>` не
навигирует, а по `<span>` внутри `<button>` не отправляет форму: те же две
строки.

## Масштаб

Маркера в `timeout_audit.py` намеренно нет: остаточные id
`html/semantics/forms/the-label-element/*` ждут либо `<iframe>`
([BUG-480](BUG-480-OPEN.md)), либо реального пользовательского клика через
`test_driver` ([BUG-810](BUG-810-OPEN.md)), так что отдельного правила по
исходнику не выводится. Заводится по прямому замеру.

Вне WPT это заметно почти на каждой странице: `<label><span>…</span></label>`,
`<a><img></a>`, `<button><svg>…</svg></button>` — стандартная разметка, и на
Lumen клик по внутреннему элементу молча ничего не делает.

## Направление починки (не предписание)

Перед активацией подниматься от `nid` вверх по предкам до первого узла, у
которого таблица `_lumen_run_activation_behavior` даёт ветку, и активировать
его (ограничившись, как в спеке, путём события). Защита от рекурсии
`label → control → label` уже есть (`_lumen_click_in_progress`), её надо
сохранить.

## Как проверить фикс

1. `tests/wpt/.venv/bin/python tests/wpt/verify_navigation_form_import_gaps.py
   --variant label-click-activates` — ожидаются `cb-click` и
   `change checked=true`.
2. WPT: `run_report.py --all --root html/semantics/forms/the-label-element --recursive`.

## Починено (P1, 2026-08-25)

**Заявка называла один дефект, а их было два — на разных путях клика.**
JS-половина ровно та, что описана выше. Вторая нашлась при чтении соседнего
кода: `forms::classify_click` (`crates/shell/src/forms.rs`) — путь **реального
клика мышью** — ключуется тем же способом (тег кликнутого узла) и ветки
`<label>` не имеет вовсе, то есть на живой странице клик по подписи не
переключал чекбокс даже при попадании курсором в сам `<label>`, а не только в
`<span>` внутри него. Заявка этого не видела, потому что мерила `.click()` из
скрипта; `links::find_link_href` рядом уже поднимался по предкам ради `<a>`, что
и делало дефект незаметным на ссылках.

**JS-путь.** Появился `_lumen_activation_target(nid)` — подъём по цепочке
предков (она же путь события) до первого узла, у которого таблица
`_lumen_run_activation_behavior` даёт ветку. На него переехала и pre-click
activation (переворот чекбокса), и её откат при отменённом событии: DOM §2.9
вычисляет activation target ДО диспетчеризации, и держать половины одной
последовательности на разных узлах нельзя. Оба места вызова — `click()` и
`dispatchEvent` у обёртки элемента — идут через один и тот же подъём.

**Нативный путь.** `classify_click` начинается с `activation_target(doc, node)`
(та же форма, что у `find_link_href`), а `<label>` резолвится в labeled control
до `match`: `for` → `Document::find_by_id` с проверкой на labelable, иначе первый
labelable-потомок в порядке дерева. Хит-тест приходит и на текстовый узел —
подъём это переживает, потому что смотрит на `element_name()`.

**Две границы, без которых подъём чинит одно и ломает другое.**

* Интерактивный контент без собственного поведения
  (`SELECT`/`TEXTAREA`/`IFRAME`/`EMBED`/`OBJECT`) **останавливает** подъём.
  HTML LS §4.10.20 требует, чтобы `<label>` ничего не делал для событий,
  нацеленных в такого потомка: иначе клик по `<textarea>` внутри
  `<label for=cb>` переключал бы чекбокс.
* Проверка `disabled` переехала внутрь активации (обе стороны). Раньше её
  хватало в `click()`/на кликнутом узле, потому что активировался он же; с
  подъёмом target бывает предком, и без проверки клик по `<span>` внутри
  `<button disabled>` отправлял бы форму.

**Замер после фикса** (`verify_navigation_form_import_gaps.py`, Windows,
dev-release, `--seconds 6`):

| проба | было | стало |
|---|---|---|
| `label-click-activates` | `clicked-span checked=false active=`, событий нет | `cb-click`, `change checked=true`, `clicked-span checked=true` |
| `label-click-direct` (контроль) | полностью работает | не изменился |

**Тесты:** `crates/js/tests/cases/activation_target.rs` (9) и 8 новых в
`forms.rs`. A/B на неизменённом `dom.rs`: красные ровно 4 «предковых» случая,
остальные 5 (контроль, барьер, `disabled`, отмена, отсутствие цели) зелёные и до
фикса — то есть каждый утверждает свою половину, а не общий факт.

**Не входило.** `active=` пуст и после фикса, и это верно: синтетический клик по
спеке фокус не переносит (шаги synthetic click activation фокусировки не
содержат), фокус двигает только настоящий пользовательский клик. Событий `click`
и `change` на контроле при **нативном** клике по-прежнему не видно из JS — шелл
переворачивает атрибут сам и не диспатчит их; это отдельный, более старый
разрыв нативного пути и к activation target отношения не имеет.
