# BUG-543: Custom element constructors never run — neither on upgrade of a pre-existing element nor on `document.createElement()` of an already-defined tag

**Статус:** OPEN
**Дата:** 2026-08-03
**Компонент:** js (`crates/js/src/dom.rs` — `_lumen_ce_upgrade_element` at line ~7463, `customElements.define`/`document.createElement` element-construction path)
**Найден:** WPT-RUN-3 срез 29 (`ROADMAP.md`) — массовый прогон `css/css-shadow` (`part/*.html`)

## Механизм

`_lumen_ce_upgrade_element(el, entry)` (`dom.rs:7463`), the function that
runs when an already-parsed element is upgraded after a matching
`customElements.define()`, only:
1. marks the element `__ceUpgraded__`, and
2. calls `entry.ctor.prototype.connectedCallback` if present.

It never invokes `entry.ctor` (the class constructor itself) — the step
where, per the HTML Custom Elements spec, the element's own construction
logic (`super()` plus whatever the subclass constructor does, e.g.
`this.attachShadow(...)`, initial property setup) is supposed to run.
`connectedCallback` is a *separate*, later lifecycle hook and is not a
substitute for the constructor.

Confirmed live via three incremental `--dump-layout` probes
(`.tmp/probe3.html` .. `.tmp/probe5.html`):
- A class with a `constructor` that appends a marker string, `define()`d
  while a matching `<my-ce>` tag already exists in the DOM: the marker never
  appears — constructor did not run on upgrade.
- The same class, with `document.createElement('my-ce2')` called *after*
  `define()`: the marker still never appears — constructor does not run on
  fresh-element creation either. This is a **broader gap than just the
  upgrade path** — custom element constructors appear to never execute
  under any code path.
- `attachShadow` called from inside such a constructor (the common pattern
  for a shadow-DOM-backed custom element) therefore never runs, so
  `element.shadowRoot` stays `null` for every custom element that relies on
  the constructor to attach its shadow root, rather than doing so in
  `connectedCallback`.

## Симптом

`css/css-shadow/part/*.html` (`complex-matching.html`, `different-host.html`,
`double-forward.html`, `invalidation-complex-selector-forward.html`, and
others) use a shared `support/shadow-helper.js::installCustomElement()`
that defines exactly this pattern — `attachShadow` inside the constructor.
`getElementByShadowIds()` walks `host.shadowRoot` and throws `"No
shadowRoot found: i=N id=<id>. Host was [object Object]"` — **33 subtests**
this slice, all `css-shadow`/`part/`.

## Как исправить (не входит в объём P2)

`_lumen_ce_upgrade_element` needs to actually invoke the registered
constructor against the existing element instance (the standard "upgrade an
element" algorithm re-runs the constructor with `this` bound to the
existing DOM node — not a trivial `new Ctor()`, since the element already
exists; needs whatever mechanism V8 uses elsewhere in this codebase for
binding an existing native node to a JS class instance, if any — otherwise
this needs new plumbing). Separately, `document.createElement()` for an
already-registered tag name needs to run the constructor as part of element
creation (the "create an element" algorithm, spec step 6.1: for a
custom-element definition with a synchronous flag, run the constructor
directly). Both gaps likely share a fix once the "run constructor against
existing/new node" primitive exists.
