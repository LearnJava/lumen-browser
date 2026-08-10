# BUG-360 — event-handler content attributes (`onclick="…"`, `onencrypted="…"`, …) never fire, and live elements' `dispatchEvent` ignores the `on<type>` IDL property

**Статус:** FIXED 2026-08-09
**Компонент:** js (`crates/js/src/dom.rs:5929` — live-element `dispatchEvent`; `dom.rs:3813` `_lumen_dispatch`, `dom.rs:3831` `_lumen_dispatch_bubble`, `dom.rs:3869` `_lumen_dispatch_rich` — все три читают только `_lumen_listeners`; контраст: `dom.rs:3391` `EventTarget.prototype.dispatchEvent`, который `on<type>` как раз вызывает)
**Найден:** P2, WPT-VENDOR-encrypted-media (2026-07-28), `run_report.py --all --root encrypted-media --recursive`, тест `media-element-event-handler-attributes.html`

## Симптом

Ни одна форма event-handler-атрибута HTML не работает на живом документе. Подтверждено
вне WPT `--dump-layout`-пробами (`.tmp/probe-em2.html`, `.tmp/probe-em4.html`),
на обычном `<div>` и обычном `onclick` — то есть это не EME-специфика:

```html
<div id="d1" onclick="window.hitClick=1"></div>
```

```
P1 onclick type: undefined          // d.onclick — свойства нет вовсе
P2 hitClick: undefined              // d.dispatchEvent(new Event('click')) — не сработал
P3 setAttr onclick type: undefined  // el.setAttribute('onclick', …) — то же самое
P4 hitClick2: undefined
```

При этом сам атрибут в DOM лежит — теряется только его компиляция в обработчик:

```
Q1 getAttribute onclick: window.hitA=1
```

Второй, независимый слой той же поломки — даже **вручную присвоенный** обработчик
не вызывается на живом элементе (`.tmp/probe-em4.html`):

```
R1 typeof d.onclick after assign: function   // присвоение прижилось
R2 dispatch returned: true | hitB: undefined | hitC: 1
```

`hitC` — это `addEventListener`-слушатель, он сработал; `hitB` — присвоенный
`d.onclick = fn`, он проигнорирован. То есть на живых элементах `on<type>` как
IDL-атрибут не существует ни в одну сторону: ни запись из HTML, ни чтение при
диспатче.

Побочно замечено той же пробой (`.tmp/probe-em5.html`): `HTMLElement.click()` на
живом элементе отсутствует — `b2.click is not a function`. Отдельного бага не
заводилось, ни один тест категории на это не упал.

## Причина

Живой элемент (`_lumen_make_element`) отдаёт `dispatchEvent`, который уходит в
нативный реестр слушателей и больше никуда:

```js
// crates/js/src/dom.rs:5929
dispatchEvent: function(evt) {
    if (!evt) return true;
    evt.target = this; evt.currentTarget = this;
    return _lumen_dispatch(nid, evt);
},
```

а `_lumen_dispatch` (`dom.rs:3813`) читает исключительно `_lumen_listeners[nid + ':' + type]` —
таблицу, которую наполняет только `addEventListener`. То же верно для обоих
бабблящих путей: `_lumen_dispatch_bubble` (`dom.rs:3831`) и `_lumen_dispatch_rich`
(`dom.rs:3869`) — а именно они вызываются шеллом на настоящий пользовательский
ввод (`_lumen_dispatch_mouse_event`, `dom.rs:3906`; `_lumen_dispatch_key_event`).
Поэтому `<button onclick="…">` мёртв и при живом клике мышью, не только при
синтетическом `dispatchEvent`.

Контраст внутри одного и того же шима: обобщённый `EventTarget.prototype.dispatchEvent`
(`dom.rs:3373`, для не-DOM таргетов) `on<type>` как раз вызывает —

```js
// crates/js/src/dom.rs:3391
var onprop = 'on' + type;
if (typeof this[onprop] === 'function') {
    try { this[onprop].call(this, event); } catch (e) {}
}
```

— то есть нужное поведение в шиме уже написано, просто живая DOM-ветка его не
использует.

Второй недостающий кусок — компиляция самого атрибута. Во всём workspace нет ни
одного места, которое смотрело бы на атрибуты с префиксом `on`:
`grep -rn "starts_with(\"on\")" crates/ --include=*.rs` — пусто, и в
`crates/shell/`/`crates/dom/` строка `onclick` не встречается вообще. Парсер
кладёт `onclick` в общий мешок атрибутов и на этом всё заканчивается.

## Масштаб

В категории `encrypted-media` этим объясняется единственный не-HTTPS тест, который
дошёл до выполнения и упал по движковой причине: `media-element-event-handler-attributes.html`,
4 сабтеста — `FAIL onencrypted handler set from parser` (упал ещё раньше, на
соседнем гэпе named access, см. ниже), `TIMEOUT onencrypted handler set via
setAttribute()` и 2 `NOTRUN`. Числа здесь
маленькие только потому, что 96 из 99 id категории до выполнения не дошли вовсе
(HTTPS-порт).

Вне WPT масштаб принципиально другой: **inline-обработчики — базовая форма
интерактивности HTML**. `<button onclick="…">`, `<form onsubmit="…">`,
`<body onload="…">`, `<img onerror="…">`, `<a onclick="return false">` — всё это
на Lumen не выполняется. Любая страница, не использующая `addEventListener`
(а это весь «старый веб», серверные шаблоны, админки, значительная часть
почтовых рассылок и лендингов), выглядит статичной картинкой.

Тем же тестом вскрыт **третий, отдельный гэп — named access on the Window
object** (HTML LS §7.3.3): элемент с `id="testElement"` не появляется как
`window.testElement`. Проба (`.tmp/probe-em.html`):

```
NAMED-ACCESS audio: undefined | window.testElement: undefined
NAMED-ACCESS div:   undefined | window.myDiv:       undefined
```

`document.getElementById` при этом работает. Именно на этом упал сабтест
«handler set from parser» (`ReferenceError: testElement is not defined`), не
дойдя до собственно проверки обработчика. Это соседняя, но не та же поломка;
чинить логично одним заходом.

## Возможный фикс (не реализован в этой сессии)

1. Заставить живую ветку диспатча вызывать `on<type>`: в `_lumen_dispatch`
   (`dom.rs:3813`), `_lumen_dispatch_bubble` (`dom.rs:3831`) и
   `_lumen_dispatch_rich` (`dom.rs:3869`) после прохода по `_lumen_listeners`
   для текущего `nid` дёргать `el['on' + type]`, повторив уже существующий блок
   `dom.rs:3391`. Порядок по спеке: обработчик из атрибута занимает позицию в
   списке слушателей в момент своей установки, но для первого приближения
   достаточно «после явных слушателей узла».
2. Хранилище `on<type>`: сейчас свойство живёт на transient-обёртке из
   `_lumen_make_element` (проба показала, что обёртка кэшируется — `b === b2` —
   но полагаться на это нельзя). Правильнее хранить рядом с `_lumen_listeners`,
   в таблице `nid → {type: fn}`, и отдавать через геттер/сеттер на прототипе
   элемента.
3. Компиляция контент-атрибута: при установке атрибута с префиксом `on`
   (и при парсинге, и в `setAttribute`) собирать функцию из тела атрибута —
   в терминах спеки это `new Function('event', body)` с `this`, привязанным
   к элементу, — и класть в хранилище из п. 2. Снятие атрибута
   (`removeAttribute`) должно обработчик убирать.
4. Named access on Window — отдельным шагом: `window[id]` для элементов с `id`
   (и `name` для формоподобных). Дешёвая реализация — `Proxy`/`has`-ловушка на
   глобале или ленивое `Object.defineProperty` при регистрации `id` в DOM.

Не чинилось в этой сессии — P2-wpt вендорит и обследует, кодовые фиксы — полоса
P3 (`CLAUDE.md`, developer assignments).

**WPT-RUN-3 срез 10 (`css/css-variables`, 2026-08-02):** ещё один пример —
`test_variable_legal_values.html` использует `<body onload="run()">`;
`setup({explicit_done: true})` вызывается, но `run()` никогда не
исполняется (тот же дефект: `on<type>`-атрибут не компилируется в
обработчик), поэтому `done()` не вызывается вовсе — харнес зависает TIMEOUT
с нулём зарегистрированных `test()`. `.ini`:
`tests/wpt/metadata/css/css-variables/test_variable_legal_values.html.ini`
(`expected: TIMEOUT`).

**WPT-RUN-3 срез 11 (`css/css-overflow`, 2026-08-02):** крупнейшее расширение
на сегодня — 25 файлов, весь `check-layout-th.js`-протокол (`<body
onload="checkLayout('.container')">`) целиком мёртв на этой категории.
Диагностировано живой пробой: `--mcp-live-port` подтверждает, что
`document.readyState` уже `"complete"`, `typeof window.checkLayout ===
"function"`, атрибут `onload="checkLayout('.container')"` присутствует на
`<body>` — но вручную вызванный `window.checkLayout('.container', false)`
отрабатывает мгновенно (`0.01s`) без исключений, то есть сама функция
рабочая, просто браузер никогда её не вызывает через content-атрибут. Все 25
файлов — `scrollable-overflow-transform-{001..010}.html`,
`scrollable-overflow-with-nested-elements-{001..005}.html`,
`scrollable-overflow-{padding,padding-inline,padding-input,self-collapsing,
textarea,zero-one-axis,float,empty-child-box,replaced-element-001}.html`.
`.ini`: `expected: TIMEOUT` на уровне файла (ноль зарегистрированных
`test()`).

**Срез 22 (`css/css-rhythm/computedstyle`, 2026-08-03):** те же 2 файла, тот
же механизм — `<body onload="checkLayout('.test')">`, ноль
зарегистрированных `test()`, TIMEOUT на уровне файла:
`block-level-replaced-elements-affected-by-block-step-size.html`,
`inline-level-replaced-elements-not-affected-by-block-step-size.html`.
Изначально пропущены в этом же срезе при первом прогоне (не попали в
собственный `<summary>`-блок HTML-отчёта, тот же артефакт вложенных
meta-строк, что и у BUG-519's файлов) — найдены только повторным прогоном
после коммита `.ini`, подтверждающим 0 unexpected passes. `.ini` под
`tests/wpt/metadata/css/css-rhythm/computedstyle/` для обоих файлов,
`expected: TIMEOUT` на уровне файла.

## Срез 24 (`css/css-scrollbars`, 2026-08-03)

9 more files, all `setup({explicit_done:true})` + `<body onload="performTest()">`:
`scrollbar-color-003/004/005.html`, `scrollbar-width-001/002/003/004/015/016.html`.
Harness never reports (0 subtests registered), each pays the full ~10s
wptrunner timeout. `.ini` under `tests/wpt/metadata/css/css-scrollbars/`,
`expected: TIMEOUT` file-level only (no subtests to suppress).

## Срез 33 (`css/css-sizing`, 2026-08-03)

48 more files (`stretch/*`, `keyword-sizes-on-*`, `contain-intrinsic-size/*`)
— largest single-slice extension yet, dominating this category's TIMEOUT
cluster (48 of 69). Root-caused via `--processes 1 --limit 3` on a
sub-sample + grep for `onload\s*=` in the vendored source, same method as
срез 32. `.ini` under `tests/wpt/metadata/css/css-sizing/`, file-level
`expected: TIMEOUT`.

## Фикс (P3, 2026-08-09)

Реализованы все 3 пункта из «Возможный фикс» (named access on Window —
п.4 — не входил в скоуп, ушёл отдельным BUG-384, см. ниже):

**Хранилище (п.2).** Новая таблица `_lumen_on_handlers` (`crates/js/src/dom.rs`,
рядом с `_lumen_listeners`), ключ `String(nid) + ':' + type` (без префикса
`on`) → текущая функция-обработчик. Табличное хранение, а не expando на
кэшированной обёртке элемента, выбрано так, чтобы бабблящий диспатч мог
проверить «есть ли обработчик у этого предка» по одному nid, не вызывая
`_lumen_make_element` на каждом шаге ради простой проверки. `_lumen_gc_collect`
чистит `_lumen_on_handlers` по тому же префиксу nid, что и `_lumen_listeners`
— обработчик живёт ровно столько же, сколько остальное per-nid состояние узла
(шелл вызывает `_lumen_gc_collect` только для отсоединённых узлов с нулём
живых JS-ссылок).

**Компиляция контент-атрибута (п.3).** `_lumen_compile_inline_handler(body)` —
`new Function('event', body)` в try/catch; непарсящееся тело даёт `null`
(обработчик не ставится), а не бросает. Компиляция триггерится в трёх местах:
(a) при первой постройке живой обёртки элемента (`_lumen_build_element`)
сканированием `_lumen_get_attr_names(nid)` на префикс `on` — покрывает и
атрибуты из HTML-парсера, и `setAttribute`, случившийся до первого JS-доступа
к узлу; (b) из `setAttribute`, когда имя атрибута начинается с `on`; (c) из
`removeAttribute` — очищает обработчик. `getAttribute('onclick')` по-прежнему
отдаёт исходный текст атрибута — компиляция не трогает атрибутную таблицу.

**Диспатч (п.1).** Все три живых пути диспатча (`_lumen_dispatch`,
`_lumen_dispatch_bubble`, `_lumen_dispatch_rich` — последние два обслуживают
реальный пользовательский ввод через `_lumen_dispatch_mouse_event`/
`_lumen_dispatch_key_event`) вызывают `on<type>` после явных
`addEventListener`-слушателей на той же цели, тем же порядком, что и
`EventTarget.prototype.dispatchEvent` (`dom.rs:406-430`) и уже существовавший
`_lumen_dispatch_focus_event` для focus/blur.

**`el.on<type>` как IDL-свойство.** Curated список `_LUMEN_EVENT_HANDLER_ATTRS`
(GlobalEventHandlers HTML LS §8.1.7.2 + `onencrypted`/`onwaitingforkey`) —
геттер/сеттер на каждой живой обёртке элемента (не Text/Comment), читающие и
пишущие через `_lumen_get_on_handler`/`_lumen_set_on_handler`. Не входящие в
список имена (`onFooBar`) по-прежнему компилируются и диспатчатся из
контент-атрибута (диспатч читает таблицу напрямую по имени, не через
accessor), но не получают JS-свойство на элементе — соответствует спеке
(произвольные имена не являются настоящими IDL-атрибутами).

**`<body onload>` → `window.onload` (HTML LS §8.1.7.3).** Единственный
форвардящийся атрибут из «Window-reflecting body element event handler set» —
это именно тот сценарий, который блокирует `check-layout-th.js`
(`<body onload="checkLayout(...)">`, срезы 11/22/24/33 выше) и
`test_variable_legal_values.html`. Форвард безопасен без риска двойного
срабатывания: `load` никогда не диспатчится через баблинг по узлам, только
через отдельный цикл слушателей в `_lumen_apply_ready_state('complete')`.
Остальные четыре имени того же спекового набора (`onblur`/`onerror`/
`onfocus`/`onresize`/`onscroll`) остались обычными локальными обработчиками
элемента — узкое известное отклонение от спеки, не покрытое ни одним
известным падением этого бага.

**Побочно замеченный гэп из симптома** (`HTMLElement.click()` отсутствует на
живом элементе) — не чинился, отдельного бага не заводилось (как и в
исходном отчёте).

11 новых регресс-тестов в `dom::tests::v8_inline_event_handlers` (компиляция
из атрибута, диспатч через `dispatchEvent` и через `_lumen_dispatch_bubble`,
`setAttribute`/`removeAttribute`, присвоенный `el.onclick = fn`, непарсящееся
тело, `<body onload>`-форвард и его порядок относительно `_lumen_gc_collect`).
`cargo test -p lumen-js --features v8-backend --lib` 2530/2530 зелёных,
`cargo clippy -p lumen-js --features v8-backend --all-targets -- -D warnings`
чисто. Подтверждено `--dump-layout`-пробой на исходном repro из симптома
(`.tmp/probe-bug360.html`): все 4 прежде-`undefined` значения теперь корректны
(`onclick` — `function`, оба клика реально срабатывают).

Named access on Window (третий гэп из «Масштаб») в скоуп не входил —
отслеживается отдельно как [BUG-384](BUG-384-FIXED.md).
