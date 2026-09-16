# BUG-611: `HTMLLinkElement.relList` not implemented

**Статус:** FIXED 2026-09-16 (дрейф трекера)
**Компонент:** js (`crates/js/src/shim/web_api_shim_tail_b.js` — `HTMLLinkElement.prototype`/`HTMLAnchorElement.prototype` `relList`)
**Найден:** P2, WPT-VENDOR-html-misc, 2026-08-04

## Симптом

```
FAIL link element supports a rel value of "manifest". - Cannot read properties of undefined (reading 'supports')
```
(`links/manifest/link-relationship/link-rel-manifest.html`:
`document.createElement("link").relList.supports("manifest")` throws
because `relList` itself is `undefined`)

## Причина

HTML LS defines `HTMLLinkElement.relList` (and the equivalent on
`HTMLAnchorElement`/`HTMLAreaElement`/`HTMLFormElement`) as a live
`DOMTokenList` reflecting the space-separated `rel` content attribute, with
a `.supports(token)` method that validates a token against the element's
list of supported link types (`"manifest"` being one, per the Web App
Manifest spec's registration into that list). Lumen has `link.rel`/
`link.rev` as plain string reflection but no `relList` accessor at all —
`link.relList` is `undefined`, so `.supports(...)` throws a `TypeError`
before the actual "is manifest a supported rel value" check can even run.

## Масштаб

1 file, 1 subtest confirmed here (`<link>`). Likely the same gap on
`<a>`/`<area>`/`<form>` `relList`, not checked in this slice.

## Ревизия P3 2026-09-16 (дрейф трекера)

Заявка описывала состояние до [BUG-826](BUG-826-FIXED.md) (FIXED 2026-08-25):
чиня доставку `<link rel=preload|modulepreload|prefetch>`, тот срез попутно
завёл `link.relList`/`a.relList` (`_lumen_make_rel_list` в
`crates/js/src/shim/web_api_shim_tail_b.js:1868` — без `supports('preload')`
ни один тест семейства preload не доходил до предмета), но указатель на
этот баг не был снят.

Живая проверка: `_LUMEN_LINK_REL_TOKENS` (`web_api_shim_tail_b.js:1856`)
включает `'manifest'`, так что исходный симптом
(`document.createElement("link").relList.supports("manifest")`) больше не
бросает и возвращает `true`. Юнит-тест
`dom::tests::v8_webworker::link_rel_list_supports_and_reflects`
(`crates/js/src/dom/tests/v8_webworker.rs:658`) уже покрывает
`relList.supports`/`.contains`/`.add` на `<link>` и проходит
(`cargo test -p lumen-js --features v8-backend link_rel_list_supports_and_reflects`
— 1 passed). Точечного фикса не потребовалось, указатель снят из
`STATUS-P3.md`.
