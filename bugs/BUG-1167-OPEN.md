# BUG-1167 — youtube: колбэки custom elements Polymer падают на `this._attributeToProperty`/`b.Aa is not a function`

**Статус:** OPEN
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
