# BUG-836 — activation behaviour ищется на самом кликнутом узле, а не на ближайшем активируемом предке: клик по `<span>` внутри `<label>` (или внутри `<a>`/`<button>`) не делает ничего

**Статус:** OPEN
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
