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
