# BUG-1213 — Trusted Types: SVG `<script>` `href`/`xlink:href` не считается sink'ом TrustedScriptURL

**Статус:** OPEN
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

`getAttributeType(tagName, attribute)` знает только `on*`, `iframe srcdoc` и
`script src` и не принимает namespace элемента/атрибута (TT §4.4 — сигнатура
`getAttributeType(tagName, attribute, elementNs, attrNs)`). Таблица спецификации
содержит SVG `script` + `href` (атрибут без namespace и в XLink) →
`TrustedScriptURL`; у Lumen HTML-`script` `src` и SVG-`script` `href` различить
нечем, потому что проверка идёт по нижнему регистру имени тега без namespace.

## Направление починки

Передавать в `_lumen_tt_get_compliant_attribute_value` namespace элемента и
атрибута (оба вызова в `web_api_shim_mid.js`: `setAttribute`/`setAttributeNS`) и
расширить `getAttributeType` до спецификационной сигнатуры: SVG-`script` + `href`
(ns `null` или XLink) → `TrustedScriptURL`, sink `SVGScriptElement href`. Attr-путь
после BUG-689 идёт через те же `setAttribute(NS)`, отдельной правки не требует.
Проверка — WPT `trusted-types/TrustedTypePolicyFactory-getAttributeType-svg.html`,
`…-namespace.html` и два файла выше; после починки переписать их `.ini` ратчетом.
