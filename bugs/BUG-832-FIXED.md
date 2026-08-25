# BUG-832 — `hashchange` доставляется синхронно из сеттера `location.hash`: слушатель, повешенный строкой ниже, событие уже не увидит

**Статус:** FIXED 2026-08-25
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

## Починено — P1, 2026-08-25

`_lumen_fire_hashchange` больше не зовёт слушателей: она строит событие и
кладёт задачу в `_lumen_timers` с `nesting: 0` (тот же приём, что у
`_ro_schedule_initial` и `Animation.prototype._fire`) — не через `setTimeout`,
потому что кламп §8.6 на 4 мс относится к вложенности таймеров, а не к
задаче, которую ставит движок. Сам вызов слушателей выделен в
`_lumen_dispatch_hashchange`, и правки все три точки вызова
(`_lumen_set_location_hash`, `_lumen_navigate_or_fragment`,
`_lumen_deliver_popstate`) не потребовали.

Микрозадача, как и предполагала заявка, не годится: она выполняется до
возврата в цикл событий, то есть до конца текущего скрипта — а слушатель в
`004`/`005`/`006`/`007` вешается именно внутри этого скрипта, строкой ниже
присваивания.

Две детали формы, которые решают, фикс это или нет.

**Объект события строится в момент постановки**, а не в момент доставки. Два
присваивания в одном обороте цикла (`location.hash='a'; location.hash='b'`)
обязаны дать два события со своими парами `oldURL`/`newURL` — если строить
событие в задаче, оба прочитают `location`, уже уехавший на `#b`. Ровно на
этом стоит `006.html`, и юнит-тест
`bug832_two_hash_writes_in_one_turn_deliver_both_url_pairs_in_order` держит
именно порядок и содержимое пары, а не факт доставки.

**Исключение слушателя остаётся на своём месте.** Заявка предполагала, что с
переходом на очередь `try {} catch(e) {}` надо будет разворачивать в
`window.onerror`, но к моменту починки этого уже не требовалось: сплошная
зачистка `dom.rs` в рамках [BUG-591](BUG-591-FIXED.md) (2026-08-23) заменила
здесь оба голых `catch` на `_lumen_report_exception`. Поэтому пер-слушательный
`try`/`catch` сохранён (спека §8.5 требует «report and continue» — упавший
слушатель не должен уносить следующих), а не отдан наружу циклу таймеров,
который сообщил бы об исключении и прервал бы доставку остальным.

### Замер

Живая проба, `--seconds 6`, dev-release, Windows (три варианта разом):

| вариант | получено |
|---|---|
| `nav-hashchange-late` | `hash-set`, `listener-attached`, **`late-hashchange old=…/.pnfi-nav-hashchange-late.html new=…#pnfi-late`** |
| `nav-hashchange` (контроль) | `onhashchange-prop`, `hashchange type=hashchange hash=#pnfi`, `after-hash` |
| `nav-hashchange-chain` (контроль) | `chain-hashchange 1..3` |

WPT, A/B на одном коммите и одном профиле (`LUMEN_PROFILE=dev-release
run_report.py --all --root html/browsers/browsing-the-web/scroll-to-fragid
--recursive`, правка снята через `git stash` и шелл пересобран):

| id | до | после |
|---|---|---|
| `004.html` | TIMEOUT (10.05 с) | FAIL (0.08 с) — остаток ниже |
| `005.html` | TIMEOUT (10.14 с) | **PASS** 1/1 |
| `006.html` | FAIL `oldURL property first update` | **PASS** 1/1 |
| `007.html` | TIMEOUT (60.16 с — у файла `timeout=long`) | **PASS** 1/1 |
| категория | 19/23 harness OK, 4/29 сабтестов | **22/23**, **7/29** |

Заявка называла 3 id; их четыре — `006.html` числился FAIL, а не TIMEOUT,
потому что его слушатель ловил *второе* событие как первое (первое терялось
целиком) и падал на `oldURL`. Это тот же дефект, просто с другой стороны:
маркер `hashchange-listener-too-late` его не брал, так как страница всё-таки
что-то печатала.

### Остаток (не входило)

`004.html` теперь падает на `assert_equals(e.target, window)` — у события,
доставленного окну, `target` пуст. Это не тайминг, а общее свойство
диспетчеризации в шиме, уже заявленное в
[BUG-873](BUG-873-OPEN.md) («у события на `document` `e.target === null`»);
отдельного бага не завожу.

Не трогалось и остаётся верным для пробы: фрагментная навигация из скрипта
обновляет `location`/историю/`hashchange`, но не просит шелл о визуальной
половине — `:target` и прокрутка к цели по-прежнему только у настоящего клика
мышью ([BUG-833](BUG-833-FIXED.md), остаток). `popstate` при обходе истории
доставляется синхронно, как и раньше: очередь ему тоже положена по спеке, но
это другой механизм и другой баг ([BUG-886](BUG-886-OPEN.md) — запись
`pushState(state, "")` вообще не доезжает до стека шелла).
