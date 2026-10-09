# BUG-1577 — `ShadowRoot.getElementById` ищет по документу, `ShadowRoot.appendChild(DocumentFragment)` вставляет сам фрагмент: шаблонный custom element не строится

**Статус:** OPEN
**Заведён:** 2026-10-09 (P2, WPT-RUN-14 срез 26, `css/css-page` + `css-animations` + `css-transitions` + `css-shadow` + `css-borders` + `css-scroll-snap`)
**Область:** js (`crates/js/src/shim/web_api_shim_mid.js:4836` `ShadowRoot.prototype.getElementById`, `:4840` `ShadowRoot.prototype.appendChild`; у `DocumentFragment` нет `getElementById`)

## Симптом

Две независимые поломки, обе на пути «пользовательский элемент с шаблоном», который WPT `css-shadow` использует почти везде:

1. `ShadowRoot.getElementById(id)` ищет по всему документу и не находит узел внутри shadow tree; `DocumentFragment.getElementById` отсутствует.
2. `ShadowRoot.appendChild(fragment)` (а также `append`, `insertBefore(fragment, null)`, `replaceChildren(fragment)`) вставляет сам `DocumentFragment` как ребёнка, не разворачивая его. У обычного `Element.appendChild(fragment)` разворачивание работает (BUG-1440 описывает только соседние методы `Element`).

## Проба

`--dump-layout` + `console.log`:

| вызов | у нас | ожидается |
|---|---|---|
| `r = h.attachShadow({mode:"open"}); r.innerHTML = "<span id=a>x</span>"; r.getElementById("a")` | `null` | `<span id=a>` |
| `typeof DocumentFragment.prototype.getElementById` | `undefined` | `function` |
| `d.appendChild(tpl.content.cloneNode(true))` для `Element` (`<style>`, `<b>`, `<i>`) | `STYLE,B,I` | то же |
| `r.appendChild(tpl.content.cloneNode(true))` для `ShadowRoot` | `#document-fragment` (1 ребёнок), во фрагменте остались 3 | `STYLE,B,I` |
| `r.append(frag)`, `r.insertBefore(frag, null)`, `r.replaceChildren(frag)` | то же: `#document-fragment` | `STYLE,B,I` |
| `r.append(...frag.childNodes)` | 3 ребёнка | 3 |
| `getComputedStyle(r.querySelector("#part")).color` после `appendChild(clone)` шаблона с `<style>span{color:green}</style>` | `""` | `rgb(0, 128, 0)` |
| `template.innerHTML` для `<template>` с детьми | `""` | разметка содержимого |

A/B (временная правка `tests/wpt/css/css-shadow/part/support/shadow-helper.js`, откатана): `run_corpus.py --prefixes css/css-shadow/part` — зелёных 2 из 48 → 8 из 48; остальные упираются в отсутствие `::part` (ДОРАБОТКА → SHADOW-PARTS, BUG-1578).

## Как найдено

WPT-RUN-14 срез 26: `css/css-shadow/part/*` — `No element found: i=1 id=part. Root was [object ShadowRoot]` (26 id), а после обхода — пустой `getComputedStyle().color`.

## Что делать

В `ShadowRoot.prototype.getElementById` искать по поддереву shadow root (как `querySelector`); добавить `DocumentFragment.prototype.getElementById`; в `ShadowRoot.appendChild`/`append`/`insertBefore`/`replaceChildren` разворачивать `DocumentFragment` по ветке `__isDocumentFragment__`, как в `Element.appendChild`. `template.innerHTML` — отдельная находка (геттер пуст), в BUG-1074/1440 не входит.

## Как проверить

Проба выше; `css/css-shadow/part/different-host.html`.
