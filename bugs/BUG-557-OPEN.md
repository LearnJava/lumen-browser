# BUG-557: live global `document` object has `appendChild` but no `removeChild`/`insertBefore`/`replaceChild`

**Статус:** OPEN
**Дата:** 2026-08-04
**Компонент:** js (`crates/js/src/dom.rs:7125` — the `var document = {…}` literal behind the live global `document`)
**Найден:** P2, WPT-RUN-3 срез 39 (`css/cssom-view`), 2026-08-04

## Симптом

```
FAIL CSSOM View - 7 - element.offsetWidth detatches correctly
  document.removeChild is not a function
```

`htmlelement-offset-width-001.html` calls `document.removeChild
(document.documentElement)` on the live global document and gets a
`TypeError` instead of the node being detached (or, per spec, `NotFoundError`
if it were somehow not a child).

## Причина

The live global `document` literal (`dom.rs:7125`, ~276 lines) defines
`appendChild` (`dom.rs:7277`, inside the literal) but has no
`removeChild`/`insertBefore`/`replaceChild` at all — `grep -n
"removeChild\|insertBefore\|replaceChild" crates/js/src/dom.rs` finds these
only on: the ordinary element wrapper (`_lumen_build_element`,
`dom.rs:5826`), `DocumentFragment` (`dom.rs:4396`), and `CharacterData`
(`dom.rs:4480`, throws by design). The live `document` object was apparently
never given the rest of the `Node` mutation interface, only the one method
(`appendChild`) that happened to be needed elsewhere.

Same subsystem, same "two independently-written document objects with
non-overlapping holes" pattern already flagged by
[BUG-358](BUG-358-FIXED.md) (live document missing metadata attributes:
`characterSet`/`URL`/`compatMode`/…) and
[BUG-415](BUG-415-FIXED.md) (the *detached* document from
`createHTMLDocument`/`createDocument` missing the same `Node` methods, plus
HTML accessors) — this is the third, distinct hole in the same pair of
objects: the **live** document additionally lacks `removeChild`/
`insertBefore`/`replaceChild` specifically (as opposed to BUG-358's
metadata-attribute gap). Worth fixing together with BUG-358/BUG-415 by one
shared document-object builder, per BUG-415's own recommendation.

## Масштаб находки

1 file / 1 subtest this slice (`htmlelement-offset-width-001.html`), but
`document.removeChild`/`insertBefore`/`replaceChild` on the live document is
a basic enough API that any WPT test using it to reset document state
between assertions will hit the same wall.

## Что нужно

Give the live `document` literal the same `removeChild`/`insertBefore`/
`replaceChild` implementations the ordinary element wrapper already has
(`dom.rs:5826` area) — ideally as part of the shared builder BUG-415
proposes rather than a fourth independent copy.

---

## Перезамер 2026-09-09 (P6, задача E2E-4): не 1 сабтест, а любая страница Next.js

Стенд — [`samples/e2e4-hydration`](../samples/e2e4-hydration/README.md), настоящий
React 18.3.1, живое окно через `--mcp-live-port`, http одного origin.

**`ReactDOM.hydrateRoot(document, …)` — форма, которой гидрируется каждая страница
Next.js 14 App Router, — вешает вкладку намертво.** React ловит исключение,
повторяет попытку гидрации и падает на том же месте по кругу:

```
[JS error] TypeError: t.removeChild is not a function     × 130 738 (за ~30 с)
[JS error] Uncaught TypeError: t.removeChild ...          × 130 738
wait{document_ready} → "automation command timed out"     (30 с)
stderr-журнал прогона: 1 176 686 строк
```

Воспроизведено трижды подряд (правило §9 `docs/probe-method.md`). Контроль на том же
стенде: `hydrateRoot(<div id=root>, …)` проходит целиком — фиберы навешиваются
(`__reactFiber$…` на кнопке), маркеры Suspense `<!--$-->` разбираются штатно.
То есть ломает именно корень-`document`, а не гидрация как таковая.

### Дыра шире, чем три метода

`document` (nodeType 9, `docapi.html` стенда) не носит бо́льшую часть интерфейса
`Node`:

| есть | нет |
|---|---|
| `appendChild`, `contains`, `hasChildNodes`, `compareDocumentPosition`, `getRootNode`, `childNodes`, `nodeType`, `nodeName`, `addEventListener`/`removeEventListener`/`dispatchEvent` | `removeChild`, `insertBefore`, `replaceChild`, `cloneNode`, `normalize`, `isEqualNode`, `isSameNode`, `firstChild`, `lastChild`, `parentNode`, `textContent` |

Плюс `document instanceof Node === false` (при живом `Node` в globalThis) и
`document.documentElement.parentNode !== document` — обратно к документу приходит
свежая обёртка, а не тот же объект. У `documentElement` и `body` всё это на месте,
то есть дефект ровно в литерале.

Тот же корень, что у [BUG-1046](BUG-1046-OPEN.md) (`TreeWalker` с корнем `document`
не обходит ничего, потому что читает `document.__nid__`, которого нет) — почему
починку и стоит делать общим строителем документного объекта, как просит BUG-415.

### Что стоит за этим дефектом (разведка, не измерение)

С заглушками вместо трёх методов (`?lookahead=1` на стенде — семантика заведомо
неверная, no-op) зависание уходит, но гидрация всё равно деградирует: React
сообщает recoverable #418 (разметка не совпала) и #423 (корень переключается на
клиентский рендер), `h1` остаётся без фибера. Форма дерева при этом верная
(`document`: doctype + `HTML`; `body`: `DIV` + три `SCRIPT`; `#app`: `H1`,
`<!--$-->`, `P`, `<!--/$-->`), геттер `firstChild` (`?lookahead=2`) картину не
меняет. Второй дефект отдельным номером **не заводится**: он замерен только поверх
заведомо неверных заглушек, и до починки этого бага отличить его от их артефакта
нельзя. Проверять повторно сразу после фикса.
