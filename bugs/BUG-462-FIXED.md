# BUG-462: `Node.prototype.contains` missing on live (attached) DOM nodes

**Статус:** OPEN
**Дата:** 2026-08-02
**Компонент:** js (`crates/js/src/dom.rs` — живая обёртка `Element`/`Document`, тот же
`_lumen_build_element`/handwritten `var document = {…}` литерал, что в
[BUG-358](BUG-358-OPEN.md)/[BUG-367](BUG-367-FIXED.md))
**Найден:** WPT-RUN-2 (`ROADMAP.md`, `docs/tasks/p2-wpt-runner-throughput.md`) — первый
живой прогон вендоренного `/resources/testdriver.js` через новую поддержку
`test_driver.click()` (`executorlumen.py`) на `css/css-display/display-contents-pseudo-click-target.html`.

## Симптом

```
FAIL Clicking a display: contents pseudo-element targets that element -
  promise_test: Unhandled rejection with value: object "TypeError: elementDocument.contains is not a function"
Error
    at get_stack (<anonymous>:4802:21)
    ...
    at Test.<anonymous> (<anonymous>:764:29)
```

Стек ведёт в вендоренный (немодифицированный) `/resources/testdriver.js`,
`getPointerInteractablePaintTree`:

```js
function getPointerInteractablePaintTree(element) {
    let elementDocument = element.ownerDocument;
    if (!elementDocument.contains(element)) {   // <- бросает здесь
        return [];
    }
    ...
```

`element` — обычный, присоединённый к живому дереву элемент обычной страницы (не
detached-документ). `element.ownerDocument.contains` не определён вовсе —
`'contains' in document` false, а не сломанный геттер (тот же класс проверки, что
у остальных находок в этом файле).

## Почему это не дубликат уже заведённых багов

- [BUG-415](BUG-415-FIXED.md) фиксирует отсутствие `contains`/`removeChild`/… на
  **отсоединённом** документе (`createHTMLDocument`/`new Document()`) — другой
  строитель (`_lumen_build_detached_document`), другой объект.
- [BUG-367](BUG-367-FIXED.md) документирует, что `Node.prototype` в живом дереве несёт
  только `constructor`/`hasChildNodes` (все прочие члены — собственные свойства
  инстанса), но не перечисляет `contains` явно как отсутствующий метод — эта
  находка называет конкретный метод и конкретный воспроизводимый WPT-репро.
- `grep -n "\.contains\s*="` по `crates/js/src/dom.rs` не находит ни одной
  реализации `Node.contains`/`Document.contains`/`Element.contains` где-либо в
  шиме — отсутствует целиком, не только на отсоединённом документе.

## Влияние вне WPT

`Node.contains()` — стандартный, часто используемый метод (проверка "элемент X
внутри контейнера Y", клик-снаружи/focus-trap паттерны, сам `testdriver.js`).
Отсутствие блокирует **любой** WPT-тест, использующий `test_driver.click()` на
элементе (`getPointerInteractablePaintTree` вызывается на каждый клик через
testdriver — не только на `display-contents-pseudo-click-target.html`), то есть
маскирует находки по реальному предмету тех тестов, аналогично тому, как BUG-384
маскировало часть `focus`-категории (см. `feedback_green_test_can_mask_broken_feature`
класс проблемы — тут наоборот, ложный FAIL от инфраструктурного гэпа, а не ложный
PASS, но тот же принцип «читать текст каждого FAIL, а не только считать»).

## Репро

```bash
export LUMEN_PROFILE=dev-release MSYS2_ARG_CONV_EXCL='*'
tests/wpt/.venv/Scripts/python.exe tests/wpt/run_smoke.py \
  --binary "$(pwd)/target/dev-release/lumen.exe" \
  /css/css-display/display-contents-pseudo-click-target.html
```

Or interactively: any live page, `document.contains(document.body)` → `TypeError:
document.contains is not a function`.

## Что нужно

Add `contains(other)` to the live `Node`/`Element`/`Document` wrapper(s) — walk
`other`'s ancestor chain (via `parentNode`/`getRootNode().host` for shadow
boundaries, per DOM Standard §4.4 `Node.contains`) checking for identity with
`this`; `null`/non-Node `other` → `false`. Natural to land alongside a BUG-367-style
prototype consolidation, but a narrow instance-level shim (matching the existing
per-instance-property pattern the rest of the live wrapper uses) unblocks this
sooner if the bigger prototype refactor is not imminent.
