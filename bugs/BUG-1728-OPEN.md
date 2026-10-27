# BUG-1728 — содержимое `<template>` теряется: `innerHTML` вложенного `<template>`, геттер `template.innerHTML`, `cloneNode(true)`, `importNode`

**Статус:** OPEN
**Заведён:** 2026-10-27 (P1, WPT-SHADOW-SCOUT)
**Область:** js/dom (`crates/js/src/v8_runtime/install/dom_core.rs` `_lumen_set_inner_html`/`import_node`, `dom_helpers.rs::serialize_node`, `cloneNode`/`importNode` в `web_api_shim_mid.js`)

## Симптом

Проба `lumen --dump-layout` (документный парсер `<template>` разбирает правильно, остальные пути — нет):

```
d = div; d.innerHTML = '<template><i></i></template>';
d.firstChild.content.childNodes.length                 → 0   (ожидание 1: фрагментный парсер не кладёт детей в content)
t = template; t.innerHTML = '<b>1</b>'; t.innerHTML    → ""  (ожидание "<b>1</b>"; t.content при этом непуст)
t.outerHTML                                            → "<template></template>"
t.innerHTML = '<b>1</b><b>2</b>'; c = t.cloneNode(true); c.content.childNodes.length   → 0 (ожидание 2)
div-с-template.cloneNode(true).firstChild.content.childNodes.length                   → 0
document.importNode(t, true).content.childNodes.length → 0
```

DOM LS §4.5 «cloning steps» для `template` копируют `content`; HTML LS §13.3 сериализация `template` идёт по `content`, а не по детям.

## Как найдено

WPT-SHADOW-SCOUT (`docs/wpt-vendor-notes/shadow-dom.md`, кластер E): общий хелпер `shadow-dom/resources/shadow-dom.js::createTestTree` клонирует дерево с `<template data-mode>` и собирает из `template.content` теневые корни; клон приходит с пустым content, `ids` не содержат элементов теневого дерева → «Cannot read properties of undefined (reading 'assignedNodes' / 'dispatchEvent' / 'addEventListener')» в 18 id (`slots.html`, `slotchange.html`, `imperative-slot-api*.html`, `event-composed*.html`, `event-post-dispatch*.html`, `slots-fallback*.html` …) — 12/153 сабтестов. Тот же дефект ломает шаблонные библиотеки (Lit: `importNode(template.content, true)` при клонировании шаблона).

## Что делать

(1) Фрагментный парсер (`innerHTML`/`setHTMLUnsafe`/`insertAdjacentHTML`): `<template>` получает `content`, как в `tree_builder.rs`; (2) сериализатор `innerHTML`/`outerHTML` обходит `content` у `template`; (3) `cloneNode`/`importNode` клонируют `content` (глубоко — только при `deep`). Проверить, что `template` внутри `<select>`/`<table>` при фрагментном разборе не ломается.

## Как проверить

Пробы выше дают ожидаемое; `run_corpus.py --prefixes shadow-dom --processes 4` — ids кластера E уходят из «Cannot read properties of undefined» (дальше они упрутся в [BUG-1729](BUG-1729-OPEN.md), [BUG-1730](BUG-1730-OPEN.md)); отдельно `run_corpus.py --prefixes html/semantics/scripting-1/the-template-element`.
