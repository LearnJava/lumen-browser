# BUG-1228 — `setHTMLUnsafe` не разбирает Declarative Shadow DOM

**Статус:** OPEN
**Заведён:** 2026-09-30 (найден при BUG-1064)
**Область:** `_lumen_set_inner_html` (`crates/js/src/v8_runtime/install/dom_core.rs`), `import_node` (`dom_helpers.rs`).

## Симптом

```js
w.setHTMLUnsafe('<span><template shadowrootmode=open shadowrootserializable=""></template></span>');
w.firstElementChild.shadowRoot   // null — ожидание ShadowRoot
w.innerHTML                      // '<span><template shadowrootmode="open" …></template></span>'
```

Фрагментный разбор `innerHTML`/`setHTMLUnsafe` идёт без DSD, а `import_node` не переносит
`shadow_roots` из временного документа. Документный парсер (`tree_builder.rs`) DSD умеет.

## Масштаб

Декларативная половина `shadow-dom/declarative/gethtml.html`: 3264 FAIL-подтеста.

## Ожидание

`setHTMLUnsafe` (Element, ShadowRoot, `Document.parseHTMLUnsafe`) создаёт теневые корни по
`shadowrootmode`/`delegatesfocus`/`serializable`/`clonable` (HTML LS §13.2.6.4.?, «attach a shadow root»);
`innerHTML` — нет.

## Дополнение WPT-SHADOW-SCOUT (2026-10-27, [`shadow-dom.md`](../docs/wpt-vendor-notes/shadow-dom.md))

Пробы `lumen --dump-layout`: (1) `innerHTML`/`setHTMLUnsafe` **выбрасывают** `<template shadowrootmode>` целиком, а не оставляют `<template>`: `d.innerHTML='<a><template shadowrootmode=open><i></i></template></a>'` → `<a></a>` (спека для `innerHTML` оставляет обычный `<template shadowrootmode="open">` в DOM, это 106 сабтестов `declarative-shadow-dom-opt-in.html` и 340 `…-attachment.html`); (2) у `HTMLTemplateElement` нет IDL-отражения `shadowRootMode`/`shadowRootDelegatesFocus`/`shadowRootClonable`/`shadowRootSerializable`/`shadowRootSlotAssignment` (`undefined`); (3) `Document.parseHTMLUnsafe` существует, но корни не создаёт; (4) см. [BUG-1728](BUG-1728-OPEN.md) — фрагментный парсер не заполняет `content` вложенного `<template>`, чинить вместе. Масштаб по баллам: 21 id, потолок 16 баллов (0.024 п.п. корпуса) — сабтестов много, id мало.
