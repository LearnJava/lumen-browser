# BUG-851 — `<details>`: скриптовое изменение `open` не порождает `toggle` вовсе, а клик по `<summary>` переключает атрибут дважды — состояние не меняется, но событие о смене приходит

**Статус:** FIXED 2026-08-25 (P1)
**Заведён:** 2026-08-22 (WPT-RUN-6, срез 24 — живой замер, маркер `details-toggle-not-fired`)
**Область:** `crates/js/src/dom.rs:15522` (единственная точка диспатча `toggle` — слушатель `click` на `document`, ветка `tag === 'summary'`), `crates/js/src/dom.rs:15377` (`_lumen_run_activation_behavior`, ветка `SUMMARY` — второе переключение того же атрибута), отсутствие «attribute change steps» для `open`
**Владелец:** P1 (`lumen-js` + `lumen-shell`). Заведён P2 в ходе WPT-задачи.

## Симптом

Два независимых дефекта одного элемента.

**1. Скриптовое изменение `open` молчит.** HTML LS §4.11.1 требует, чтобы
смена содержимого атрибута `open` (свойством, `setAttribute`, `removeAttribute`
или парсером) ставила задачу «details toggle event task» и та диспатчила
`toggle`:

```js
d.open = true;                 // ничего
d.setAttribute('open', '');    // ничего
d.removeAttribute('open');     // ничего
```

**2. Клик по `<summary>` переключает `open` дважды.** Событие приходит, но
состояние возвращается на место:

```js
d.addEventListener('toggle', () => console.log(d.open, d.hasAttribute('open')));
s.click();                     // внутри обработчика: true true
console.log(d.open);           // сразу после click(): false
```

То есть `<details>` не открывается ни одним доступным скрипту способом, а
единственное отправленное событие сообщает о переходе, которого не произошло.

## Прямое измерение

`tests/wpt/verify_frame_load_media_gaps.py --variant details-toggle
--variant details-summary-click` (2026-08-22, dev-release, Linux, коммит
`c583a90b4`, `--seconds 5`, страница жива — 9 тиков):

| действие | ожидалось | получено |
|---|---|---|
| `d.open = true` | `toggle` (oldState closed → open) | тишина |
| `d.removeAttribute('open')` | `toggle` | тишина |
| `d.setAttribute('open', '')` | `toggle` | тишина |
| `summary.click()` (закрытый) | `toggle` + `open === true` после клика | `toggle` (closed→open), **`open === false`, `hasAttribute('open') === false`, в `outerHTML` атрибута нет** |
| второй `summary.click()` | `toggle` (open→closed) | снова «closed→open», состояние снова не изменилось |

Замер `--variant click-attr-write` показывает, что дело не в откате записей
вообще: `setAttribute`/`className`, сделанные из обычного `click`-слушателя,
переживают диспатч (`caw-after`/`caw-later` видят их).

## Причина (локализована чтением кода)

Атрибут переключают **две** независимые точки, и обе срабатывают на один
скриптовый клик:

```js
// dom.rs:15522 — слушатель click на document, он же диспатчит toggle
if (wasOpen) { _lumen_remove_attr(pid, 'open'); }
else         { _lumen_set_attr(pid, 'open', ''); }
...
_lumen_dispatch(pid, toggleEvt);

// dom.rs:15377 — activation behaviour, вызывается ПОСЛЕ диспатча из
// HTMLElement.prototype.click() (dom.rs:15417) и dispatchEvent (dom.rs:4701)
if (_lumen_has_attr(parent, 'open')) _lumen_remove_attr(parent, 'open');
else _lumen_set_attr(parent, 'open', '');
```

Первая точка меняет атрибут во время диспатча (поэтому обработчик `toggle`
видит новое состояние), вторая возвращает его обратно после. Ни одна из них не
привязана к изменению атрибута как такового, поэтому путь «скрипт меняет
`open`» не диспатчит ничего.

Побочно: событие отправляется синхронно, тогда как спека ставит задачу, —
`toggleEvent.html` проверяет и это. Интерфейс события (`ToggleEvent` вместо
`Event`) — отдельный баг [BUG-578](BUG-578-FIXED.md).

## Масштаб

Маркер `details-toggle-not-fired` в `tests/wpt/timeout_audit.py` — **1 id**
остатка снимка WPT-RUN-5
(`html/semantics/interactive-elements/the-details-element/toggleEvent.html`),
но внутри него 9 из 11 подтестов в состоянии NOTRUN/TIMEOUT, и разделение
ровно по механизму: все «… should fire a toggle event» висят, а
«Setting open=false on a closed 'details' element should **not** fire a toggle
event» проходит. Родственный id
`the-summary-element/anchor-with-inline-element.html` висит по другой причине
([BUG-837](BUG-837-FIXED.md)).

## Направление починки (не предписание)

Перенести переключение `open` в activation behaviour (одну точку), а диспатч
`toggle` — в шаги изменения атрибута `open`, поставленные в очередь задачей.
Тогда оба пути — клик и скрипт — обслуживаются одним механизмом, а двойное
переключение исчезает само.

## Как проверить фикс

1. `tests/wpt/.venv/bin/python tests/wpt/verify_frame_load_media_gaps.py
   --variant details-toggle --variant details-summary-click` — ожидаются
   `details-toggle` на каждое изменение `open` и совпадение состояния до/после
   `click()`.
2. WPT: `run_report.py --all --root html/semantics/interactive-elements` —
   `toggleEvent.html` должен перестать быть TIMEOUT.

---

## Починено (P1, 2026-08-25)

Переключение `open` стало **одной** точкой, а событие — шагами изменения
атрибута, как и предлагало «направление починки». Слушатель `click` на
`document` удалён целиком; поведение активации `<summary>`
(`_lumen_run_activation_behavior`) меняет атрибут обычной записью, а `toggle`
ставится задачей из шагов изменения `open`, куда ведут **все** пути: свойство
`open`, `setAttribute`/`removeAttribute`/`toggleAttribute`, разметка парсера и
нативный клик мышью. Механизм — обёртки над `_lumen_set_attr`/`_lumen_remove_attr`
(та же форма, что у `_mo_notify`), поэтому новый способ записи атрибута не может
случайно обойти событие.

### Заявка называла два дефекта, их оказалось шесть

Первые два — из заявки. Остальные четыре найдены **замером**, а не чтением:
одноразовой юнит-пробой до правки (печатала фактическое поведение через
`panic!`) и A/B-прогоном обеих затронутых категорий WPT.

1. **Скриптовая запись `open` не порождала `toggle`** (заявка). Проба «до»:
   `open=true`, `removeAttribute`, `setAttribute` — `events=0` на все три.
2. **`summary.click()` переключал атрибут дважды** (заявка). Проба «до»:
   `after summary.click(): attr=false events=1`.
3. **Нативный клик мышью тоже переключал дважды — и `<details>` не
   открывался в живом окне.** Заявка мерила `.click()` из скрипта и этого не
   видела. `main.rs` сначала шлёт в JS `_lumen_dispatch_mouse_event(…, 'click')`,
   тот доходит до слушателя на `document` и ставит `open` (проба «до»:
   `after native click JS half: attr=true`), а следом ветка
   `FormClickAction::ToggleDetails` переключает атрибут **в шелле** и шлёт
   **второй** `Event('toggle')`. Итог — состояние на месте, два события. То есть
   `<details>` не открывался вообще никак, включая обычный пользовательский клик.
4. **Событие приходило без `target`.** `_lumen_dispatch` цели не ставит
   ([BUG-873](BUG-873-OPEN.md)), а страница со слушателем на нескольких
   `<details>` иначе не может их различить: `name-attribute.html` проверяет
   `event.target === element` на каждом из четырёх. Дефект вылез только в
   прогоне: тест ушёл TIMEOUT → **ERROR**, потому что ассерт бросал из
   слушателя, а после [BUG-591](BUG-591-FIXED.md) исключение долетает до
   harness. Правка — та же, что [BUG-838](BUG-838-FIXED.md) сделал для
   `load`/`error`.
5. **`<a>` без `href` останавливал подъём к активационной цели.** Тоже нашёл
   прогон: `anchor-without-link.html` дал 2/2 → **0/2**. Таблица
   `_LUMEN_ACTIVATABLE_TAGS` из [BUG-837](BUG-837-FIXED.md) считает `A`
   активируемым по тегу, но HTML LS §4.6.1 говорит, что `<a>` без `href` —
   заполнитель: ни поведения активации, ни интерактивного контента. Раньше это
   не было видно, потому что удалённый слушатель на `document` искал `summary`
   по предкам сам и мимо таблицы. Теперь подъём проходит `<a>` насквозь — и
   вместе с этим закрылся `anchor-with-inline-element.html`, который заявка
   относила к BUG-837.
6. **Второй `<summary>` тоже переключал `<details>`.** HTML LS §4.11.2 отдаёт
   поведение активации только **первому** `<summary>`-ребёнку.

Заодно закрыт [BUG-578](BUG-578-FIXED.md) — без него правка не измерима:
`toggleEvent.html` в собственном `testEvent()` проверяет
`Object.getPrototypeOf(evt) === ToggleEvent.prototype` у каждого события, а
такого класса в шиме не было вовсе. Добавлен `ToggleEvent` (readonly-аксессоры
`oldState`/`newState`, WebIDL-конверсия `DOMString`: явный `undefined` —
«член отсутствует», `null` → строка `'null'`; плюс `source` из Popover API L2),
и на него переведены все четыре точки диспатча popover'а; `beforetoggle`
popover'а стал `cancelable` и его отмена теперь прерывает показ/скрытие, как
требует спека (раньше `preventDefault()` там не делал ничего).

### Что важно знать о форме правки

- **`toggle` — задача, а не синхронный диспатч.** По статье после записи в
  `open` ещё ничего не доставлено; юнит-тест обязан прокрутить
  `_lumen_tick_timers()`. Задача пишется прямо в `_lumen_timers` с `nesting: 0`
  (форма `_ro_schedule_initial`/`_lumen_fire_hashchange`): зажим §8.6 в 4 мс —
  про вложенность таймеров, а не про задачу, которую движок ставит за страницу.
- **Две записи в одном обороте дают ОДНО событие.** Спека переиспользует
  `oldState` уже поставленной задачи и снимает её, а не ставит вторую, поэтому
  `open=true; open=false` приходит как `closed`→`closed` — это ровно то, что
  проверяют t2/t6/t8.
- **Флип нативного клика остался в шелле.** JS ему только сообщают, что
  изменилось (`_lumen_details_native_toggled`): атрибут должен лежать в
  документе до `relayout_form()`, идущего следом, — это то же рассуждение
  ADR-016 M2.2c-3, что стояло в прежнем комментарии.
- **Парсерная половина — скан в конце разбора**, как у
  [BUG-826](BUG-826-FIXED.md)/[BUG-838](BUG-838-FIXED.md): разметка мимо хука
  записи атрибутов не проходит, поэтому `<details open>` из разметки должен
  получить `closed`→`open` отдельно. Запись `_details_known_open` не даёт
  выстрелить дважды по элементу, который скрипт уже трогал.

### Замер

Юнит: 14 тестов `v8_details_dialog_popover::details_*` (было 5), полный
`lumen-js` — 3235 зелёных.

A/B по двум категориям WPT (dev-release, Windows, бинарники «до» и «после»
собраны из одного дерева через `git stash`):

| категория | harness OK | подтесты |
|---|---|---|
| `html/semantics/interactive-elements` | 100/147 → **101/147** | 56/446 → **81/449** |
| `html/semantics/popovers` | 80/103 → **81/103** | 999/3886 → **1036/3886** |

Изменившиеся файлы (регрессий нет ни одной):

| тест | было | стало |
|---|---|---|
| `the-details-element/toggleEvent.html` | TIMEOUT 1/11 | TIMEOUT **10/11** |
| `the-summary-element/activation-behavior.html` | OK 2/9 | OK **9/9** |
| `the-summary-element/anchor-with-inline-element.html` | TIMEOUT 0/2 | **OK 5/5** |
| `the-details-element/name-attribute.html` | TIMEOUT 2/18 | TIMEOUT **4/18** |
| `the-details-element/details-toggle-source.html` | OK 0/3 | OK **2/3** |
| `popovers/toggleevent-interface.html` | OK 0/39 | OK **36/39** |
| `popovers/popover-events.html` | **ERROR** 0/6 | **OK** 0/6 |
| `popovers/popover-toggle-source.html` | OK 0/7 | OK **1/7** |

### Остаток

- `toggleEvent.html` остаётся TIMEOUT из-за одного подтеста: `<details open>`
  внутри `new DOMParser().parseFromString(...)`. Такой документ не проходит ни
  через хук записи атрибутов, ни через скан конца разбора — заведено
  [BUG-919](BUG-919-OPEN.md).
- `beforetoggle` у `<details>` не диспатчится вовсе (у popover'а — есть):
  `details-toggle-source.html` и `popover-toggle-source.html` упираются в
  отсутствующий `event.source` и `command`-атрибуты.
- Три подтеста `toggleevent-interface.html` из оставшихся — не про
  `ToggleEvent`, а про базовый конструктор `Event`: `new Event()` без аргумента
  не бросает `TypeError`, а `null`/`undefined` в качестве типа дают `''` вместо
  `'null'`/`'undefined'` (общее для всех 26 классов событий шима).
- Отмена нативного клика из обработчика (`preventDefault()`) по-прежнему не
  мешает шеллу переключить `<details>`: результат JS-диспатча там не читается —
  давняя дыра нативного пути, отмеченная в остатке BUG-837.
