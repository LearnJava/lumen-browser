# BUG-297 — `Element.prototype.setAttributeNS` is unimplemented

**Статус:** OPEN
**Компонент:** js (`crates/js/src/dom.rs`, `_lumen_build_element`)
**Найден:** P2-wpt, 2026-07-17, first real WPT `TEST_END` result via `tests/wpt/run_smoke.py` (closing [BUG-295](BUG-295-FIXED.md))

## Симптом

`tests/wpt/dom/nodes/Element-hasAttribute.html`'s first subtest
("hasAttribute should check for attribute presence, irrespective of namespace")
fails:

```
el.setAttributeNS is not a function
    at Test.<anonymous> (<anonymous>:7:6)
```

`setAttribute`/`getAttribute`/`removeAttribute`/`hasAttribute`/`toggleAttribute`
all exist on the element wrapper (`_lumen_build_element`, `crates/js/src/dom.rs`)
but there is no `setAttributeNS` (nor `getAttributeNS`/`removeAttributeNS`/
`hasAttributeNS`) — DOM LS §4.9.3.

## Что нужно для закрытия

Add `setAttributeNS(namespace, qualifiedName, value)` (and ideally the
`*NS` siblings for parity) to the element wrapper. Existing attribute storage
(`_lumen_set_attr`/`_lumen_get_attr`/native `Attribute` type in `lumen_dom`)
already carries a `QualName` with a namespace field (used by
`createElementNS`/`namespaceURI`) — check whether the native `_lumen_set_attr`
binding already threads a namespace through, or whether it needs a new
`_lumen_set_attr_ns` native. Re-run `tests/wpt/run_smoke.py` — DoD is both
`Element-hasAttribute.html` subtests passing.
