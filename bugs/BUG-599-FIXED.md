# BUG-599: `Node.prototype.getRootNode()` missing entirely — breaks `get_selector_array` in the vendored wptrunner testdriver shim, silently masking/hanging every `test_driver_internal.*` action

**Статус:** FIXED 2026-09-09
**Компонент:** js (`crates/js/src/dom.rs`, `Node.prototype` shared-method block — same location as `hasChildNodes`, see [BUG-574](BUG-574-OPEN.md) which already predicted this exact sibling gap for `Node.prototype.contains()`)
**Найден:** P2, WPT-VENDOR-html-interaction, 2026-08-04

## Симптом

```
FAIL Tablist child gets implied tab role - promise_test: Unhandled rejection with value: object "TypeError: current.getRootNode is not a function"
```
(`focusgroup/tentative/ax-role-inference-children.html`, 21 subtests across
the file — every `test_driver.get_computed_role(element)` call)

Also observed as a silent **TIMEOUT** rather than a rejection on tests that
call `test_driver.send_keys`/`.click` without surfacing the rejection to the
harness directly: `focus-01.html`/`focus-02.html` (async keyboard-event
tests) hang for the full 10-15s timeout instead of failing fast — same root
cause, worse symptom, harder to attribute.

## Причина

`Node.prototype.getRootNode(options)` (DOM §4.4 — returns the node's
shadow-inclusive root, or `this` if unattached) is absent end to end:
`grep -rn "getRootNode" crates/js/src/` returns zero method-definition hits.
[BUG-574](BUG-574-OPEN.md), which found the sibling `Node.prototype.contains()`
gap, already flagged `getRootNode` (along with `compareDocumentPosition`/
`isSameNode`/`isEqualNode`) as "worth checking in the same fix pass" but
didn't verify it against a concrete failure; this run provides that
verification via a different call site than `contains()`'s.

The break is in `tools/wptrunner/wptrunner/testdriver-extra.js` (vendored,
unmodified — appended onto every `/resources/testdriver.js` response, see
the `CLAUDE.md` gotcha on that static route), inside the selector-builder
used by essentially every `test_driver_internal.*` action:

```js
// testdriver-extra.js:144, get_selector()
if (element.getRootNode() == element.ownerDocument) { ... }

// testdriver-extra.js:163, get_selector_array()
current = current.getRootNode().host;
```

`get_selector_array` is on the call path of `click`, `send_keys`,
`action_sequence`, `get_computed_role`, `get_computed_label`, and
`get_accessibility_properties_for_element` — i.e. nearly every
`test_driver_internal` action a WPT test can take, not just clicks (which
already fail via BUG-574/BUG-462's `contains()` gap on a *different* call
path, `resources/testdriver.js`'s own `getInViewCenterPoint`).

## Масштаб

Because both `contains()` and `getRootNode()` sit on independent, widely-used
paths inside the vendored (unpatchable) testdriver machinery, this pair is a
plausible root cause behind an unknown share of `testdriver`-flagged
TIMEOUTs/FAILs across every already-run WPT-VENDOR category that uses
`test_driver_internal.*` beyond plain `click()` — e.g. `send_keys`,
`get_computed_role`/`get_computed_label` (accessibility-flavored tests),
`action_sequence` (drag/pointer tests). Worth a targeted re-check once both
this bug and BUG-574/BUG-462 are fixed together, since several categories'
"testdriver SKIP wall" numbers were computed before either gap was known and
may have absorbed this as unexplained noise.

In this session's slice alone: 21 subtests directly show `current.getRootNode
is not a function`, and 2 of the 21 top-level TIMEOUTs (`focus-01.html`,
`focus-02.html`) are consistent with the same failure manifesting as a hang.

---

## Уточнение 2026-09-09 (P6, дорожка E2E)

**Формулировка «missing entirely» устарела — баг починен частично и не закрыт.**
Проверка на актуальном `main`:

- `_LUMEN_WRAPPER_MEMBERS.getRootNode` реализован —
  `crates/js/src/shim/web_api_shim_mid.js:7444` (обходит родителей до корня,
  отдаёт `document` для присоединённого узла);
- у фрагментов свой `getRootNode` — там же, 2646, внутри
  `_lumen_make_document_fragment`;
- **дырка ровно одна:** рукописный литерал `document`
  (`web_api_shim_mid.js:9422`) не наследует `Node.prototype`, и `getRootNode`
  в него не скопирован.

Это тот же класс, что [BUG-327](BUG-327-FIXED.md) (`hasChildNodes`) и
[BUG-732](BUG-732-FIXED.md) (`compareDocumentPosition`): оба чинились
копированием метода в этот же литерал, и комментарий рядом с ними прямо
объясняет, почему `Node.prototype` сюда не достаёт. Остаток бага — одна
строка плюс регрессионный тест.

## Подтверждение на реальном приложении

Живая проба против внешнего стенда (Keycloak + Next.js 14 App Router),
2026-09-09: react-dom вызывает `getRootNode()` на контейнере приложения, а
контейнер у App Router — сам `document`, поэтому гидрация падает с
`Minified React error #446`. С добавленным `document.getRootNode` ошибка
уходит, и открывается следующий блокер — [BUG-982](BUG-982-OPEN.md)
(теряются comment-узлы, на которых React 18 держит границы Suspense).
То есть починка этого бага сама по себе гидрацию не включает, но без неё
дальше не пройти.

---

## Починка 2026-09-09 (P6, дорожка E2E)

`getRootNode: function() { return this; }` добавлен в литерал `document`
(`crates/js/src/shim/web_api_shim_mid.js`, рядом с `contains` и
`compareDocumentPosition` — теми же двумя методами, что чинились так же по
[BUG-327](BUG-327-FIXED.md)/[BUG-732](BUG-732-FIXED.md)). Документ — корень
собственного дерева и никогда не лежит внутри теневого, поэтому опция
`composed` ответ изменить не может.

Дырка подтверждена статически до правки: `getRootNode` в шиме существовал
только у литерала `DocumentFragment` (:2646) и в `_LUMEN_WRAPPER_MEMBERS`
(:7444), а в литерал `document` ничто члены обёрток не копирует — то есть
`typeof document.getRootNode` было `'undefined'`.

Тесты: `crates/js/src/dom/tests/v8_bug599_get_root_node.rs`, 3 штуки —
`document.getRootNode() === document` (в т.ч. с `{composed:true}` и
`undefined` аргументом), совпадение корня присоединённого узла с `document` и
с его же `ownerDocument` по **идентичности** (`testdriver-extra.js:144`
сравнивает их через `==`), и отсоединённое поддерево, которое коренится на
своей вершине, а не на документе — последний отличает настоящий обход от
константы.

**Что этой правкой НЕ закрыто и вынесено в
[BUG-1045](BUG-1045-OPEN.md):** ветка «корень = shadow-дерево». Пробой на том
же рантайме: `typeof sr.getRootNode === 'undefined'`, а узел внутри теневого
дерева получает `Element`-обёртку с `host === undefined` вместо `ShadowRoot`,
поэтому переход `current.getRootNode().host` (`testdriver-extra.js:163`)
обрывается молча. Это не однострочник — `attach_shadow` кладёт shadow-корень
без родителя, и отображения nid → host наружу нет. Туда же перенесён пункт,
который [BUG-676](BUG-676-FIXED.md) сознательно отложил «в BUG-574/BUG-599».
