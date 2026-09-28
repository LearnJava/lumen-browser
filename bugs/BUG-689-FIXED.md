# BUG-689 — `Attr` node subsystem entirely absent: `Element.attributes`, `document.createAttribute(NS)`, `Element.getAttributeNode(NS)`/`setAttributeNode(NS)` all missing

**Статус:** FIXED 2026-09-28 (P3, ветка `p3-bug689-create-attribute`)
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

## Исправление (2026-09-28, P3)

К моменту починки `Element.attributes`, `getAttributeNode(NS)`/`setAttributeNode(NS)`
уже существовали (BUG-732), но как обёртки на одно обращение: каждый доступ создавал
новый `Attr`, `setAttributeNode` возвращал «живой» старый `Attr`, показывающий новое
значение, а `document.createAttribute(NS)` не было вовсе. Всё в
`crates/js/src/shim/web_api_shim_mid.js`:

- **`Attr`** — состояние в `WeakMap` `_lumen_attr_state` (`nid` владельца или `null`,
  qualified name, namespace, снимок значения), аксессоры на `Attr.prototype`. Пока
  элемент несёт атрибут, чтение идёт в него; после удаления объект хранит последнее
  значение (снимок снимается при создании, при каждом чтении и перед
  `removeAttribute`/`removeAttributeNS`/`toggleAttribute` через
  `_lumen_attr_note_removal`). `localName`/`prefix` делятся по двоеточию только у
  атрибута с namespace (`setAttribute('pre:fix')` даёт локальное имя `pre:fix`).
  Сеттер `value` у прикреплённого `Attr` идёт через `setAttribute(NS)` элемента, так
  что Trusted Types, компиляция `on*` и CE-реакции применяются как обычно.
- **`NamedNodeMap`** — методы на `NamedNodeMap.prototype` (состояние карты в
  `_lumen_nnm_impl`), именованный атрибут не перекрывает ничего на цепочке прототипов
  (WebIDL named property visibility: атрибуты `item`, `toString`); `Attr` кэшируются на
  время жизни карты, так что `el.getAttributeNode('id') === el.attributes[0]`.
  `setNamedItem(NS)` — DOM «set an attribute»: `InUseAttributeError` для чужого
  `Attr`, замена по (namespace, local name), возврат отвязанного старого;
  `removeAttributeNode` требует именно этот узел, иначе `NotFoundError`.
- **`createAttribute`/`createAttributeNS`** на живом и detached-документах: имя по
  ослабленной продукции DOM (без пробелов, NULL, `/`, `=`, `>`), нижний регистр в
  HTML-документе, validate-and-extract с четырьмя проверками `NamespaceError`.
- Попутно: `find_attr_by_namespace` (`crates/js/src/v8_runtime/dom_helpers.rs`) для
  произвольного URI ищет по `Namespace::Other` — `setAttributeNS` хранит его с
  GAP-XMLDOC среза 37, а поиск откатывался к полному имени и не находил `p:foo`;
  `get/has/removeAttributeNS` принимали `undefined` от натива («не найдено») за имя;
  «locate a namespace» отвечает на префиксы `xml`/`xmlns` сам;
  `lookupNamespaceURI`/`lookupPrefix` сравнивают `xmlns:*` по полному имени, как раньше.

Тесты — `crates/js/src/dom/tests/v8_bug689_attr_nodes.rs` (13).

WPT (dev-release, `run_smoke.py`/`run_report.py`, база — тот же слот до правки):
`dom/nodes/Document-createAttribute.html` 0→36/36, `attributes-namednodemap.html`
5→8/8, `attributes.html` 26→43/67, `Node-lookupNamespaceURI.html` ERROR 51/70 → OK
60/75. `trusted-types`, 16 файлов, трогающих Attr-API: 432→744/1192 сабтестов, их
`.ini` переписаны ратчетом. 24 бывших PASS (SVG `<script href>` через Attr-путь)
проходили только за счёт `TypeError` от отсутствующего `createAttributeNS` — реальная
причина в таблице Trusted Types, [BUG-1213](BUG-1213-OPEN.md).

Остаток `attributes.html` — не Attr-узлы: `setAttribute`/`toggleAttribute` не
валидируют имя, «первый атрибут с таким именем» при дублях из разных namespace,
регистр имён у HTML-элемента, `setAttributeNS` сливает атрибуты с одинаковым
qualified name без учёта регистра (`set_attribute_ns`).
