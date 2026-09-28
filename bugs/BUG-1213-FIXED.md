# BUG-1213 — Trusted Types: SVG `<script>` `href`/`xlink:href` не считается sink'ом TrustedScriptURL

**Статус:** FIXED 2026-09-29 (P3)
**Заведён:** 2026-09-28 (P3, при закрытии [BUG-689](BUG-689-FIXED.md))
**Область:** js (`crates/js/src/trusted_types.rs` — `getAttributeType` и
`_lumen_tt_get_compliant_attribute_value`)

## Симптом

Под `require-trusted-types-for 'script'` без default policy запись обычной строки в
`href` (или `xlink:href`) SVG-элемента `<script>` не бросает `TypeError` — ни через
`setAttribute`/`setAttributeNS`, ни через Attr-путь (`setAttributeNode(NS)`,
`NamedNodeMap.setNamedItem(NS)`, `Attr.value`/`nodeValue`/`textContent`).

WPT `trusted-types/set-attributes-require-trusted-types-no-default-policy.html` и
`trusted-types-secondary-document.html`: все сабтесты `… throws for
elementNS=http://www.w3.org/2000/svg, element=script, … attrName=href …` — FAIL
(отмечены `expected: FAIL` в `tests/wpt/metadata/trusted-types/`).

До BUG-689 восемь из них на каждом Attr-API числились PASS, но только потому, что
`document.createAttributeNS` отсутствовал и `assert_throws_js(TypeError, …)` ловил
`TypeError: … is not a function` — к Trusted Types это отношения не имело.

## Причина

`getAttributeType(tagName, attribute)` знал только `on*`, `iframe srcdoc` и
`script src` и не принимал namespace элемента/атрибута (TT §4.4 — сигнатура
`getAttributeType(tagName, attribute, elementNs, attrNs)`). Таблица спецификации
содержит SVG `script` + `href` (атрибут без namespace и в XLink) →
`TrustedScriptURL`; у Lumen HTML-`script` `src` и SVG-`script` `href` было
нечем различить, потому что проверка шла по нижнему регистру имени тега без
namespace.

## Фикс

`getAttributeType(tagName, attribute, elementNs, attrNs)` (`crates/js/src/trusted_types.rs`)
расширен до спецификационной сигнатуры:

- SVG `script` + `href` (ns `null` или XLink) → `TrustedScriptURL`, sink
  `SVGScriptElement href`;
- `on*` теперь ограничен null attribute namespace И элементом в
  HTML/SVG/MathML namespace (было: любой elementNs);
- `iframe[srcdoc]`/`script[src]` требуют элемент без namespace или в XHTML
  namespace (было: только `null`, что ломало обычные HTML-документы, где
  `namespaceURI` элементов — XHTML URI, а не `null`).

`_lumen_tt_get_compliant_attribute_value` (`trusted_types.rs`) и оба вызова в
`web_api_shim_mid.js` (`Element.setAttribute`/`setAttributeNS`) теперь
прокидывают namespace элемента (`_lumen_get_namespace_uri`) и атрибута
(`null` для `setAttribute`, реальный ns-аргумент для `setAttributeNS`).
Attr-путь (`setAttributeNode(NS)`, `NamedNodeMap.setNamedItem(NS)`,
`Attr.value`/`nodeValue`/`textContent`) идёт через тот же `setAttribute(NS)`
после BUG-689, отдельной правки не потребовалось.

Живой пробой (`--dump-layout` на минимальной странице с
`require-trusted-types-for 'script'`) подтверждено:
`trustedTypes.getAttributeType('script', 'href', SVG_NS)` → `TrustedScriptURL`,
`trustedTypes.getAttributeType('script', 'href', SVG_NS, XLINK_NS)` →
`TrustedScriptURL`, `…(SVG_NS, 'http://www.w3.org/other')` → `null`; SVG
`<script>.setAttribute('href', …)`/`.setAttributeNS(XLINK_NS, 'href', …)`
теперь бросают `TypeError`; обычный HTML `<div>` `href` не затронут.

Юнит-тесты крейта (`cargo test -p lumen-js --features v8-backend`) — 67/67
`v8_trusted_types`, включая ранее не задетые `tt_enforced_set_attribute_onclick_*`
(регрессия от первой версии фикса, где `on*` перестал матчить элементы без
явного namespace — исправлено добавлением XHTML namespace в разрешённый
список).
