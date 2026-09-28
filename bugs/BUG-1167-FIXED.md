# BUG-1167 — youtube: колбэки custom elements Polymer падают на `this._attributeToProperty`/`b.Aa is not a function`

**Статус:** FIXED 2026-09-28 (P6)
**Заведён:** 2026-09-25 (P6, перемер youtube после [BUG-1122](BUG-1122-FIXED.md))
**Область:** js (`crates/js/src/shim/web_api_shim_mid.js` — `_lumen_ce_maybe_attr_changed` вызывает
`entry.ctor.prototype.attributeChangedCallback.call(_lumen_make_element(nid), …)`,
`_lumen_ce_upgrade_element` — `connectedCallback.call(upgraded)`)

## Симптом

youtube (`probe.py lumen youtube --js g1/site.js --wait 12`, видимое окно `--maximized`,
`LUMEN_NO_ADBLOCK=1`, dev-release с BUG-1122): после того как ShadyDOM перестал включаться и
upgrade custom elements перестал падать на `this.hasAttribute`, в stderr остаются

```
[JS] LegacyDataMixin will be applied to all legacy elements.
[JS error] CE attributeChangedCallback: TypeError: this._attributeToProperty is not a function
[JS error] CE connectedCallback (upgrade): TypeError: b.Aa is not a function
```

`_attributeToProperty` — метод `PropertiesChanged`-миксина Polymer, он есть на прототипе класса
элемента. Колбэк вызывается на объекте, у которого его нет: либо обёртка узла не перешла на
прототип класса при upgrade, либо колбэк пришёл раньше конструктора (атрибут, выставленный до
`customElements.define`/upgrade). 1234 узла против 1529–1868 в Chrome, `innerText` тела пуст.

## Что сделать

Свести к минимальному репро: Polymer-подобный класс с `observedAttributes`, элемент в разметке с
атрибутом до `define`, проверить `Object.getPrototypeOf(el) === Ctor.prototype` в
`attributeChangedCallback`/`connectedCallback` и порядок реакций (HTML LS §4.13.5 «upgrade an
element»: `attributeChangedCallback` для каждого атрибута и `connectedCallback` ставятся в очередь
до конструктора, а исполняются после него — на уже сконструированном элементе).

## Причина

Две независимые, обе подтверждены: первая — тестом с принудительным GC, вторая — прогоном
настоящего `webcomponents-sd.js` youtube в рантайме с настройками страницы
(`window.ShadyDOM = {force: true, preferPerformance: true, noPatch: true}` — youtube форсирует
ShadyDOM и в Chrome).

1. **`_attributeToProperty` — обёртка custom element собиралась GC.** Кэш обёрток узлов хранит
   `WeakRef` (GAP-P3GCJSDOM). Апгрейд строит обёртку с прототипом класса, конструктор пишет на неё
   состояние, но если после этого на элемент не ссылается ни одна переменная скрипта, GC её
   собирает, и следующий `_lumen_make_element(nid)` строит обычный `HTMLElement` — без методов
   класса и без полей конструктора. `_lumen_ce_maybe_attr_changed` вызывал колбэк именно на такой
   пересобранной обёртке. Тест: `e.ping is not a function` после `force_gc_for_testing`.
2. **`b.Aa` — не custom elements, а `EventTarget`.** Стек: `c.attachShadow [as
   __shady_attachShadow]` ← `b._attachDom` ← `ready()` у `YTD-APP`. Путь ShadyDOM
   `shadyUpgradeFragment` делает `b.__proto__ = ShadowRoot.prototype; b.Aa(this, a)`, где
   `ShadowRoot` — класс ShadyDOM, который его инициализация ставит последней строкой
   (`window.ShadowRoot = yc`). До неё инициализация не доходила: `Fb()` вызывает сохранённый
   `EventTarget.prototype.addEventListener` на `window`, а шимовый метод работает только с
   `this._listeners` объектов `new EventTarget()` → `Cannot read properties of undefined (reading
   'focus')` (та же ошибка есть в логе youtube как `Uncaught TypeError`). Это первая половина
   [BUG-1123](BUG-1123-OPEN.md).

## Исправление

- `web_api_shim_mid.js`: `_lumen_ce_elements[nid]` — сильная ссылка на объект custom element с
  момента, когда для узла отработал конструктор (`_lumen_ce_build_wrapper`,
  `_lumen_ce_run_constructor`); она же заменила перечислимый expando `__ceUpgraded__` как признак
  «уже сконструирован». Колбэки connected/disconnected/attributeChanged вызываются на этом объекте,
  а не на `_lumen_make_element(nid)` или на обёртке, полученной до апгрейда. Цена: узел custom
  element больше не собирается слабо — живёт до конца документа.
- HTML LS §4.13.5 «upgrade an element», шаги 4–5: апгрейд ставит `attributeChangedCallback`
  (старое значение `null`) для каждого наблюдаемого атрибута, который уже есть на элементе, затем
  `connectedCallback`, — после конструктора. Упавший конструктор даёт состояние «failed»
  (`_lumen_ce_failed`): никаких реакций и повторного конструирования при вставке.
- `event_target_shim.js` + конец `web_api_shim_tail_b.js`: `EventTarget.prototype.addEventListener`/
  `removeEventListener`/`dispatchEvent`, вызванные на `window`, `document` или узле, делегируют в
  собственную реализацию этой цели, захваченную после сборки шима (обёртка страницы вокруг
  «нативного» метода не зацикливает делегацию); `this` = `null`/`undefined` — глобальный объект.

Тесты: `crates/js/src/dom/tests/v8_bug1167_ce_wrapper_gc.rs` (GC после `createElement`, апгрейд из
разметки + GC + смена атрибута + повторная вставка, упавший конструктор, методы `EventTarget.prototype`
на window/document/узле/чистом `EventTarget`).

## Результат на youtube

Видимое окно `--maximized`, `LUMEN_NO_ADBLOCK=1`, dev-release: обе ошибки из симптома ушли,
`ShadowRoot.name === 'yc'`, у `ytd-app` есть `_attributeToProperty`. Контент по-прежнему пуст —
следующие ошибки цепочки:

- `Uncaught TypeError: a.__shady_native_dispatchEvent is not a function` — ShadyDOM ставит
  `__shady_native_*` на `EventTarget.prototype`, а `Node.prototype` его не наследует: вторая
  половина [BUG-1123](BUG-1123-OPEN.md);
- ``CE connectedCallback (upgrade): Error: md`InjectionToken(PAGE_TOKEN)`` — DI youtube не нашёл
  провайдер страницы; вероятно, следствие предыдущей ошибки, перепроверить после BUG-1123.

Сеть до youtube во время замера была нестабильна (H2-таймауты скриптов, `curl` падал с TLS-ошибкой
35) — прогоны с `Пропуск скрипта` в stderr в счёт не шли.
