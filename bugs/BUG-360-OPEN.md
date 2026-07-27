# BUG-360 — event-handler content attributes (`onclick="…"`, `onencrypted="…"`, …) never fire, and live elements' `dispatchEvent` ignores the `on<type>` IDL property

**Статус:** OPEN
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
