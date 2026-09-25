# BUG-1171 — `TreeWalker.nextNode()` обходит поддерево `root`, а не идёт от `currentNode`: Lit не находит частей шаблона

**Статус:** OPEN
**Заведён:** 2026-09-25 (P6, при закрытии [BUG-1130](BUG-1130-FIXED.md) — перемер archive.org)
**Область:** js — `crates/js/src/shim/web_api_shim_mid_b4.js:1612` (`_TreeWalker.prototype.nextNode`
через `_tw_subtree(root)`/`indexOf(cur)`; тот же приём у `previousNode`/`nextSibling`/…)

## Симптом

archive.org (без блокировщика, `LUMEN_NO_ADBLOCK=1`, видимое окно `--maximized`, сборка с правкой
BUG-1130): `insertBefore` у shadow root больше не падает, но первая отрисовка Lit рвётся дальше —
`[unhandled-rejection] TypeError: i.getAttributeNames is not a function or its return value is not
iterable at new e (vendor-lit-CBTr2DRH.js:2:1575)`. Страница пустая: 46 узлов, `scrollHeight` 0;
Chrome — 69 узлов, 3251px, в shadow root `<app-root>` 20 детей (у Lumen 2).

Lit держит один `TreeWalker` на `document` (`z = document.createTreeWalker(document, 129)`) и для
каждого шаблона делает `z.currentNode = template.content; z.nextNode()`. По DOM §6.2 `nextNode()`
идёт от `currentNode` (сначала его первый ребёнок), и корень `root` лишь ограничивает подъём.
Lumen строит список поддерева `root` по арене и ищет в нём `currentNode`; содержимого шаблона там
нет, `indexOf` даёт `-1`, и обход начинается с `<html>` документа. Lit принимает элементы документа
за узлы шаблона и зовёт у них `getAttributeNames` — отсутствующий у элементов метод
([BUG-1136](BUG-1136-OPEN.md)).

## Репро

`.tmp/compat/g3/lit_tpl.html` в слоте `p6-work` (отдаётся `python -m http.server` с `127.0.0.1`):

```html
<!doctype html><html><body><script>
window.R = {};
var t = document.createElement('template');
t.innerHTML = '<div class="a" x$lit$="1"><span id="s">hi</span><!--?lit$1$--></div>';
var w = document.createTreeWalker(document, 129);
w.currentNode = t.content;
var seen = [], n;
while ((n = w.nextNode()) !== null) seen.push(n.nodeType + '|' + n.nodeName);
R.seen = seen;
</script></body></html>
```

**Результат:** Lumen — `["1|HTML","1|HEAD","1|BODY","1|SCRIPT"]`; Chrome —
`["1|DIV","1|SPAN","8|#comment"]`.

## Что чинить

`nextNode`/`previousNode`/`parentNode`/`*Child`/`*Sibling` — по алгоритмам DOM §6.2 от `currentNode`
через ссылки дерева (`firstChild`/`nextSibling`/`parentNode`), а не через список поддерева `root`.
Пересекается с [BUG-1164](BUG-1164-OPEN.md) п. 2–3 (тот же обход по арене). После починки
archive.org упрётся в BUG-1136 (`getAttributeNames`) — перемерять вместе.
