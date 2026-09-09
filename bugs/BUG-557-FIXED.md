# BUG-557: live global `document` object has `appendChild` but no `removeChild`/`insertBefore`/`replaceChild`

**Статус:** FIXED 2026-09-09 (P6, задача E2E-4 итерация 2)
**Дата:** 2026-08-04
**Компонент:** js (`crates/js/src/shim/web_api_shim_mid.js` — the `var document = {…}` literal behind the live global `document`; the path in the original report, `dom.rs:7125`, predates the shim's move into `.js` files)
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

## Причина (в терминах отчёта 2026-08-04)

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

---

## Починка 2026-09-09 (P6, E2E-4 итерация 2)

Всё в `crates/js/src/shim/web_api_shim_mid.js`, движок не тронут: узел-корень
документа — обычный узел арены (`_lumen_root_nid` = `lumen_dom::Document::root()`),
поэтому рёбра `document → ребёнок` мутируются теми же нативами, что и у обёртки
элемента. Отдельной реализации, как у **отсоединённого** документа
(`_lumen_build_detached_document` держит детей в JS-массиве `_children`, BUG-415),
живому документу не нужно — иначе два представления одного дерева разъехались бы.

**1. Интерфейс `Node` на литерале `document`.** `removeChild`/`insertBefore`/
`replaceChild` поверх `_lumen_root_nid` (плюс разворачивание `DocumentFragment`),
геттеры `firstChild`/`lastChild`/`parentNode`/`parentElement`/`nextSibling`/
`previousSibling`/`textContent`/`nodeValue`/`isConnected`, методы `isSameNode`/
`isEqualNode`/`normalize`/`cloneNode`. `appendChild` переписан через тот же
общий вход. Отличия от прежней «мягкой» манеры соседей (обёртка элемента молча
возвращает не-узел):

* `removeChild`/`insertBefore`/`replaceChild` бросают `NotFoundError`, когда
  узел или опорный узел не ребёнок документа, — это то, чего ждёт исходный
  WPT-сабтест;
* аргумент-не-узел — `TypeError`, иначе `replaceChild(null, old)` открепил бы
  старого ребёнка и не вставил нового;
* вставка документа в себя — `HierarchyRequestError` (DOM §4.2.3, шаг 2): без
  этой заглушки в арене появилось бы ребро-петля и любой обход дерева завис бы.

Полная проверка pre-insert (не больше одного элемента-ребёнка, запрет `Text`,
порядок doctype/элемента) **не реализована** — обёртка элемента её тоже не
делает, и вводить её на одном только документе значило бы разойтись с соседом.

**2. `document instanceof Node`.** `Object.setPrototypeOf(document, Document.prototype)`
сразу за литералом. Инертно для уже определённого: на `Document.prototype` лежит
только `constructor`, а все четыре наследуемых члена `Node.prototype`
(`hasChildNodes`, `contains`, `compareDocumentPosition`, `baseURI`) у литерала
есть собственные и перекрывают их.

**3. `documentElement.parentNode === document`.** В геттере
`_LUMEN_WRAPPER_MEMBERS.parentNode` родитель, равный `_lumen_root_nid`, теперь
отдаётся как сам синглтон `document`, а не как свежая обёртка того же узла;
`parentElement` в этом случае — `null` (родитель не элемент).

**4. Обёртка doctype (`_lumen_make_doctype`) — соседняя дыра того же обхода.**
Заведена не отдельным номером, потому что без неё правка не решает свою задачу:
doctype — **первый** ребёнок документа в standards mode, у него не было
`nextSibling`, и цикл `for (n = document.firstChild; n; n = n.nextSibling)`
(та самая форма, которой реконсилятор открывает корень-документ) обрывался на
первом же шаге, уже после починки п.1. Добавлены `nextSibling`/`previousSibling`
(через `_lumen_make_node`, чтобы обход видел DocumentType там, где он есть),
`firstChild`/`lastChild`/`textContent`/`nodeValue` (все `null` по спеке),
`cloneNode`, `isSameNode`, `isEqualNode`; равенство трёх полей вынесено в общий
`_lumen_doctype_equals`, чтобы живой doctype и его собственный `cloneNode()`
(отсоединённая форма, `_lumen_make_detached_doctype`) сравнивались одинаково.

### Проба

`.tmp/bug557-probe.html` (не коммитится — временный файл пробы), headless
`--dump-layout`, 21 утверждение, **21 ok / 0 fail**. Покрыто: идентичность
`parentNode`/`parentElement`, порядок `firstChild`/`lastChild`, три `NotFoundError`,
возврат старого ребёнка из `replaceChild`, сохранность старого ребёнка при
негодном новом, `HierarchyRequestError` на самовставку, `cloneNode(true)` →
отсоединённый документ, `isEqualNode` с собственным клоном, и две формы самого
зависания: снять `documentElement` и вернуть его, а также «очистить весь список
детей обходом `nextSibling` и собрать заново» (именно этот последний тест и
поймал дыру п.4 — до неё он останавливался на doctype).

`samples/e2e4-hydration/docapi.html` печатает теперь `function` для всех членов
таблицы выше и `document instanceof Node = true`.

### Чего проба НЕ доказала

Настоящий прогон `hydrateRoot(document, …)` не повторён: бандлы React 18 UMD
в репозиторий не вендорятся (см. README стенда), а найденный локально React —
19.2.7 без UMD-сборки, то есть стенд поднять нечем. Значит, снят механизм
зависания (`t.removeChild is not a function` больше не может возникнуть, а обход
`firstChild`→`nextSibling` доходит до конца), но не проверено, что гидрация
Next.js-формы после этого доходит до конца. Разведка итерации 1 (с заведомо
неверными заглушками) обещала следом recoverable #418/#423 — это и есть предмет
итерации 3 E2E-4, и проверять его надо на настоящем React 18.

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
