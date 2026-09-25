# BUG-1130 — `ShadowRoot` без `insertBefore`/`replaceChild` и ещё 23 членов `Node`/`ParentNode`/`DocumentOrShadowRoot`

**Статус:** FIXED 2026-09-25 (P6)
**Заведён:** 2026-09-24 (P2, разбор совместимости после прогона top100-foreign: 48 сайтов с поломкой отрисовки, видимое окно `--maximized` против Chrome 153, **без блокировщика** (`LUMEN_NO_ADBLOCK=1`); [журнал](../docs/perf/journal.md) §2026-09-24 compat). Передан P6 по решению пользователя.
**Область:** js (`crates/js/src/shim/web_api_shim_mid.js:2887` `_lumen_make_shadow_root` — `Object.create(ShadowRoot.prototype)`; `:3821-3935` — `ShadowRoot.prototype` получает только отдельные методы)

## Симптом

archive.org: `<app-root>` на Lit. Первая отрисовка шаблона вставляет маркер-комментарий в
shadow root через `insertBefore` → `[unhandled-rejection] TypeError: t.insertBefore is not a function
at tt (vendor-lit-CBTr2DRH.js:2:6707)`. Главная пустая: 45 узлов, `innerText` пуст; Chrome — высота
3210px. Замер без блокировщика, EasyList-блокировок 0.

Полный список отсутствующих членов: `insertBefore`, `replaceChild`, `normalize`, `isEqualNode`,
`isSameNode`, `childNodes`, `lastChild`, `parentNode`, `nodeType`, `nodeName`, `ownerDocument`,
`isConnected`, `moveBefore`, `firstElementChild`, `lastElementChild`, `childElementCount`,
`elementFromPoint`, `activeElement`, `getAnimations`, `styleSheets`, `delegatesFocus`,
`slotAssignment`, `clonable`, `serializable`, `onslotchange`.

## Репро

Файлы — `.tmp/compat/` в worktree аудита (`.claude/worktrees/perf-base`), отдаются `python -m http.server` с `127.0.0.1`; сравнение — `.tmp/compat/probe.py both <url>` (видимое окно, без блокировщика).

`g3/shadow.html`:

```html
<!doctype html><html><body><div id=h></div><x-el></x-el>
<script>
window.R = {};
try {
  const h = document.getElementById('h');
  const sr = h.attachShadow({mode:'open'});
  R.srType = Object.prototype.toString.call(sr);
  R.srInsertBefore = typeof sr.insertBefore;
  R.srAppendChild = typeof sr.appendChild;
  R.srFirstChild = String(sr.firstChild);
  R.protoHas = 'insertBefore' in ShadowRoot.prototype;
  R.isNode = sr instanceof Node;
  R.isDF = sr instanceof DocumentFragment;
  R.hostShadowSame = h.shadowRoot === sr;
  R.hostShadowIB = typeof (h.shadowRoot && h.shadowRoot.insertBefore);
  try { sr.insertBefore(document.createComment('x'), null); R.ibOk = sr.childNodes.length; } catch (e) { R.ibErr = String(e); }
} catch (e) { R.err = String(e); }
class XEl extends HTMLElement {
  connectedCallback() {
    const r = this.shadowRoot ?? this.attachShadow({mode:'open'});
    R.ceType = typeof r.insertBefore;
    R.ceShadowSame = this.shadowRoot === r;
    try { r.adoptedStyleSheets = []; R.ceAdoptOk = true; } catch (e) { R.ceAdoptErr = String(e); }
    R.ceTypeAfterAdopt = typeof r.insertBefore;
    R.ceFirstChild = String(r.firstChild);
    try { r.insertBefore(document.createComment(''), r.firstChild ?? null); R.ceIbOk = r.childNodes.length; } catch (e) { R.ceIbErr = String(e); }
  }
}
customElements.define('x-el', XEl);
</script></body></html>
```

**Результат:** Lumen: `srInsertBefore=undefined`, `ibErr='TypeError: sr.insertBefore is not a function'`; перечень из `.tmp/compat/g3/shadow_members.html` — 25 отсутствующих. Chrome: `function`, `ibOk=1`.

## Что сделать

DOM §4.8: `ShadowRoot : DocumentFragment : Node`, плюс миксины `DocumentOrShadowRoot` и
`ParentNode`. Вместо ручного набора на `ShadowRoot.prototype` связать его с
`DocumentFragment.prototype` → `Node.prototype` (см. баг про члены на прототипах интерфейсов) и
добавить собственные атрибуты `ShadowRoot`. Критерий: `shadow_members.html` даёт пустой список
отсутствующих; archive.org перемерить.

## Исправление (P6, 2026-09-25)

**Причина.** После BUG-1122 члены `Node` (`insertBefore`, `replaceChild`, `childNodes`,
`parentNode`, `nodeType`, `isConnected`, …) уже лежали на `Node.prototype`, и цепочка
`ShadowRoot.prototype → DocumentFragment.prototype → Node.prototype` до них дотягивалась.
Оставались: обход элементов `ParentNode` (`firstElementChild`/`lastElementChild`/`childElementCount`)
и `moveBefore` — только на `Element.prototype`; `DocumentOrShadowRoot` (`activeElement`,
`styleSheets`, `getAnimations`, `elementFromPoint(s)`, `fullscreenElement`, `pointerLockElement`)
и собственные атрибуты `ShadowRoot` (`delegatesFocus`, `slotAssignment`, `clonable`,
`serializable`, `onslotchange`) не существовали, а словарь `attachShadow()` кроме `mode` терялся.

**Что сделано** (`crates/js/src/shim/web_api_shim_mid.js`):

- `attachShadow()` сохраняет `ShadowRootInit` по nid теневого корня (`_lumen_shadow_root_init`);
  у декларативного корня записи нет — атрибуты отдают значения по умолчанию;
- `firstElementChild`/`lastElementChild`/`childElementCount`/`moveBefore` ставятся на
  `DocumentFragment.prototype` теми же дескрипторами, что у элемента (читают только `__nid__`);
- `DocumentOrShadowRoot` на `ShadowRoot.prototype`: `activeElement`/`fullscreenElement`/
  `pointerLockElement` — перенацеливание (DOM §4.2.2 retarget) и `null`, если узел вне этого
  дерева; `styleSheets` — листы реестра, чей владелец в этом теневом дереве; `getAnimations` —
  анимации документа с целью внутри корня; `elementFromPoint(s)` — хит-тест документа,
  перенацеленный на корень;
- `delegatesFocus`/`slotAssignment`/`clonable`/`serializable` (readonly) и `onslotchange`.

Сериализация `getHTML({serializableShadowRoots})` в скоуп не входила — она остаётся в
[BUG-1064](BUG-1064-OPEN.md) (теперь у него есть `ShadowRoot.serializable`/`clonable`/
`delegatesFocus`/`slotAssignment`).

**Проверка.**

- `crates/js/src/dom/tests/v8_bug1130_shadow_root_members.rs` — 8 тестов, до правки 5 из 6
  исходных красные; `cargo test -p lumen-js --features v8-backend --lib` — 4381/4381.
- `g3/shadow_members.html` (видимое окно, без блокировщика): Lumen — список отсутствующих пуст,
  Chrome — пуст. `g3/shadow.html` — все поля совпадают с Chrome (`srInsertBefore=function`, `ibOk=1`,
  `ceIbOk=1`).
- archive.org: `t.insertBefore is not a function` ушёл, но главная всё ещё пустая (46 узлов,
  высота 0; Chrome — 69 узлов, 3251px). Следующая причина — Lit обходит шаблон общим
  `TreeWalker` с `currentNode = template.content`, а `nextNode()` Lumen идёт по поддереву `root`
  от `<html>` документа и падает на `getAttributeNames` — заведено [BUG-1171](BUG-1171-OPEN.md)
  (дальше упрётся в [BUG-1136](BUG-1136-OPEN.md)).
