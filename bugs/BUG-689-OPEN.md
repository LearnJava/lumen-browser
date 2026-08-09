# BUG-689 — `Attr` node subsystem entirely absent: `Element.attributes`, `document.createAttribute(NS)`, `Element.getAttributeNode(NS)`/`setAttributeNode(NS)` all missing

**Статус:** OPEN
**Компонент:** js (`crates/js/src/dom.rs`, `WEB_API_SHIM` — `_lumen_build_element` for the `Element`
side, `_lumen_build_document`-style factory functions for the `Document` side)
**Найден:** 2026-08-09 (P2), WPT-VENDOR-trusted-types

## Симптом

```
document.createElement('div').attributes            // undefined (not a NamedNodeMap)
document.createAttribute('x')                        // TypeError: document.createAttribute is not a function
document.createAttributeNS(ns, 'x')                   // TypeError: document.createAttributeNS is not a function
document.createElement('div').getAttributeNode('x')   // TypeError: … .getAttributeNode is not a function
document.createElement('div').getAttributeNodeNS(…)   // TypeError: … .getAttributeNodeNS is not a function
document.createElement('div').setAttributeNode(attr)  // TypeError: … .setAttributeNode is not a function
```

Confirmed live via `--mcp-live-port` (`Object.getOwnPropertyNames` of both the instance and its
prototype show no `attributes` member at all — not a broken getter, the property does not exist)
and by grep: `crates/js/src/dom.rs` has zero occurrences of `NamedNodeMap`, `createAttribute`, or
`getAttributeNode` anywhere in the file.

## Причина

Lumen's attribute model is **name-only** — attributes are stored/looked up as plain
name→string pairs via the `_lumen_get_attr`/`_lumen_set_attr`/`_lumen_remove_attr`/
`_lumen_get_attr_names` natives (`dom.rs:2675-2707`), with no backing `Attr` DOM node type at
all. This was a known, deliberate scope cut recorded in passing when the `*NS` accessors were
added:

> [BUG-309-FIXED.md](BUG-309-FIXED.md): "The Attr-node variants (`getAttributeNodeNS`/
> `setAttributeNodeNS`) are intentionally omitted — the base non-`NS` `getAttributeNode`/
> `setAttributeNode` do not exist in the shim either (no Attr node objects)."

That note never got its own tracked bug — this files it, now that a WPT category's actual signal
shows the size of the gap: it is not just the four `*AttributeNode*` methods, it is the entire
`Attr`/`NamedNodeMap` half of DOM §4.9 (`Element.attributes`, `document.createAttribute()`,
`document.createAttributeNS()`), all missing for the same root cause.

`CAPABILITIES.md`'s DOM line ("Node model: Document / Doctype / Element / Text / Comment /
ShadowRoot; `QualName`, 6 namespaces, attributes.") lists "attributes" as done — true only for the
string get/set/has/remove surface, not for the `Attr` node objects DOM §4.9 also requires.

## Данные WPT

`trusted-types` (2026-08-09, run `run_report.py --all --root trusted-types --recursive`,
98/230 harness OK, 576/2465 subtests): this single root cause accounts for the two largest FAIL
clusters in the whole run —

- 447× `Cannot read properties of undefined (reading 'length')` — every test iterating
  `element.attributes.length` via the shared `support/attributes.js` helper's `findAttribute()`
  (`for (let i = 0; i < element.attributes.length; i++)`) dies before the harness even reaches the
  API under test.
- 324× `document.createAttributeNS is not a function`.
- 12× `document.createAttribute is not a function`.
- 9× `… .getAttributeNode is not a function`.
- 8× `… .getAttributeNodeNS is not a function`.

Together these account for the large majority of the category's 1889 unexpected subtests — most
of `trusted-types`' own signal (CSP/Trusted-Types enforcement) never gets exercised because the
shared attribute-iteration helper fails first.

## Направление починки

Two independently useful pieces, both additive (no change to the existing name-based
get/set/has/remove path, which many other tests already rely on):

1. **`document.createAttribute(name)` / `createAttributeNS(ns, qualifiedName)`** (`dom.rs`, next
   to `createElement`/`createElementNS` at `dom.rs:4310-4341`): construct a minimal `Attr`-shaped
   object (`name`/`localName`/`namespaceURI`/`prefix`/`value`/`ownerElement`/`specified`) not yet
   attached to any element — same shape `setAttributeNode` below would accept.
2. **`Element.prototype.attributes`** (`dom.rs`, next to `hasAttributes` at `dom.rs:2683-2685`): a
   live-ish `NamedNodeMap` built from `_lumen_get_attr_names(nid)` — array-like with
   integer-indexed `Attr`-shaped entries plus `getNamedItem`/`setNamedItem`/`removeNamedItem`/
   `item`/`length`, backed by the same name-only natives. `getAttributeNode`/`getAttributeNodeNS`/
   `setAttributeNode`/`setAttributeNodeNS` can then wrap `getAttribute`/`setAttribute` with the
   same `Attr`-shaped object.

Since the underlying storage has no notion of an `Attr` identity separate from the name/value
pair, the honest implementation is "materialize an `Attr`-shaped wrapper on demand" rather than a
truly live node graph — matches the existing `*NS` precedent (BUG-309: "namespace argument is
accepted but ignored").
