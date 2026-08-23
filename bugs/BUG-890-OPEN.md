# BUG-890 — глобального конструктора `CustomElementRegistry` нет, а вместе с ним и всей области видимости реестров: `new CustomElementRegistry()`, `createElement(..., {customElements})`, `importNode(..., {customElements})`

**Статус:** OPEN
**Заведён:** 2026-08-23 (WPT-RUN-6, срез 29 — живой замер, вариант `custom-registry`)
**Область:** js (`crates/js/src/dom.rs:6716` — `var customElements = {` — реестр существует как ОДИН объектный литерал, класса за ним нет; `grep -n "CustomElementRegistry" crates/` — ноль совпадений)
**Владелец:** P1/P3. Заведён P2 в ходе WPT-задачи, здесь не чинится.

## Симптом

Сам реестр работает: `customElements.define()` регистрирует класс,
`connectedCallback` вызывается при вставке, `get`/`whenDefined` на месте. Но за
единственным экземпляром нет интерфейса — `typeof CustomElementRegistry` даёт
`undefined`, поэтому:

* `new CustomElementRegistry()` — `ReferenceError`;
* `document.createElement(tag, {customElements: reg})` и
  `document.importNode(node, {customElements: reg})` (форма из HTML LS
  [whatwg/html#10854](https://github.com/whatwg/html/issues/10854)) не с чем
  вызвать;
* `element.attachShadow({customElements: ...})` и
  `shadowrootcustomelementregistry` — тем более.

Падение синхронное и на первой строке файла, поэтому вердикт TIMEOUT: ни один
`test()` зарегистрироваться не успевает.

## Прямое измерение

`tests/wpt/verify_cssom_svg_interface_gaps.py --variant custom-registry`
(2026-08-23, dev-release, Linux):

```
customElements = object        global-ctor = undefined
define = defined               ce-connected
upgrade-on-append = appended   get = function        whenDefined = function
new-registry        THREW CustomElementRegistry is not defined
createElement-opts  THREW CustomElementRegistry is not defined
importNode-opts     THREW CustomElementRegistry is not defined
```

## Цена по WPT

5 id снимка WPT-RUN-5 с текстом `CustomElementRegistry is not defined`, вся
папка `custom-elements/registries/`: `Document-importNode.html`,
`Document-createElement.html`, `Document-createElementNS.html`,
`scoped-registry-initialize.html`,
`scoped-registry-effective-global-registry.html` — последний до этого среза
числился за [BUG-480](BUG-480-OPEN.md) по маркеру исходника (`<iframe>` в
файле есть), хотя бросает он раньше, ещё до фрейма.

## Что дальше

Минимальный шаг, закрывающий текст ошибки, — вынести литерал в класс
`CustomElementRegistry` и опубликовать глобал, оставив `window.customElements`
его экземпляром (тесты `registries/*` пойдут дальше первой строки и станут
честными FAIL). Сама область видимости (реестр на теневое дерево) — отдельная
работа: `_lumen_ce_*`-натив знает один глобальный словарь определений.
