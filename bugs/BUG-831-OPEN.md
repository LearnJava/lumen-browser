# BUG-831 — `hashchange` доставляется синхронно из сеттера `location.hash`: слушатель, повешенный строкой ниже, событие уже не увидит

**Статус:** OPEN
**Заведён:** 2026-08-22 (WPT-RUN-6, срез 21 — найден живым замером, есть маркер `hashchange-listener-too-late`)
**Область:** `crates/js/src/dom.rs:6323` (`_lumen_set_location_hash` → `_lumen_fire_hashchange` прямым вызовом), `crates/js/src/dom.rs:6351` (`_lumen_navigate_or_fragment` — то же самое), `crates/js/src/dom.rs:6358` (`_lumen_fire_hashchange`)
**Владелец:** P1/P3 (`lumen-js`). Заведён P2 в ходе WPT-задачи, здесь не чинится.

## Симптом

```js
location.hash = "x";
addEventListener("hashchange", function () { /* никогда */ });
```

Событие уже доставлено — внутри присваивания, до того как выполнилась
следующая строка. Тот же код в обратном порядке (слушатель первым) работает
полностью корректно, включая `oldURL`/`newURL` и цепочку из нескольких
переходов.

## Прямое измерение

`tests/wpt/verify_navigation_form_import_gaps.py` (2026-08-22, dev-release,
Linux, коммит `762a0cad9`, `--seconds 6`; обе страницы живы — по 11 тиков):

| проба | получено |
|---|---|
| `nav-hashchange` (слушатель до присваивания) | `onhashchange-prop`, `hashchange type=hashchange hash=#pnfi`, `hash-set hash=#pnfi href=http://…#pnfi` |
| `nav-hashchange-late` (слушатель после) | `hash-set hash=#pnfi-late`, `listener-attached` — события нет |
| `nav-hashchange-chain` (переход из обработчика) | `chain-hashchange 1..3` — цепочка работает |

Порядок — единственная разница между первой и второй пробой: страницы
идентичны построчно.

## Причина (локализована чтением кода)

```js
function _lumen_set_location_hash(v) {           // dom.rs:6313
    …
    _lumen_location_update(newHref);
    _lumen_history_push(…); _lumen_history_push_url(…);
    _lumen_fire_hashchange(oldHref, newHref);    // dom.rs:6323 — синхронно
}
```

HTML LS §7.10.6 требует ставить `hashchange` в очередь задачи
(«queue a global task on the DOM manipulation task source»), а не звать
слушателей из сеттера. `_lumen_navigate_or_fragment` (`:6351`) — путь
`location.href=`/`assign`/`replace` — устроен так же.

## Масштаб

Маркер `hashchange-listener-too-late` — **3 id** остатка WPT-RUN-5:
`html/browsers/browsing-the-web/scroll-to-fragid/004.html`, `005.html`,
`007.html`. Все три сначала присваивают `location.hash`, а слушателя вешают
следующей строкой; в браузере со спека-совместимой очередью это рабочий
код.

Маркер намеренно требует именно такого порядка: те же две строки наоборот —
исправная страница, и матчить их означало бы забрать все фрагментные тесты
корпуса.

## Направление починки (не предписание)

Обернуть оба вызова `_lumen_fire_hashchange` в постановку задачи — в шиме
для этого уже есть `queueMicrotask`/таймерная очередь; правильнее задача, а
не микрозадача (микрозадача выполнится до конца текущего скрипта и не
поможет). Заодно `_lumen_fire_hashchange` глотает исключения слушателей
(`try {} catch(e) {}`) — с переходом на очередь их надо отдавать в
`window.onerror`, иначе они останутся невидимыми (BUG-591).

## Как проверить фикс

1. `tests/wpt/.venv/bin/python tests/wpt/verify_navigation_form_import_gaps.py
   --variant nav-hashchange-late` — ожидается `late-hashchange`.
2. WPT: `run_report.py --all --root html/browsers/browsing-the-web/scroll-to-fragid --recursive`.
