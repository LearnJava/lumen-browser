# BUG-1202 — `elementFromPoint`/`elementsFromPoint` отвечают по устаревшему дереву хит-теста: только что вставленный элемент не находится

**Статус:** OPEN
**Компонент:** js (`crates/js/src/v8_runtime/install/platform.rs:386`/`:394` — нативы `_lumen_element_from_point`/`_lumen_elements_from_point` в `install_point_hit_test`)
**Найден:** P3, попутно к [BUG-680](BUG-680-FIXED.md), 2026-09-28

## Симптом

Элемент, вставленный скриптом, в том же такте не находится хит-тестом по точке, хотя
его геометрия уже известна:

```js
const b = document.createElement('button');
b.textContent = 'Blessing...';
document.body.appendChild(b);
const r = b.getClientRects()[0];            // 8,8 79.5×23 — верно
document.elementsFromPoint(r.left + r.width / 2, r.top + r.height / 2);
// → []          (ожидается [button, body, html])
document.elementFromPoint(...)               // → null
```

Проба через `run_smoke.py` на release-бинарнике `main` (+ BUG-680), 2026-09-28: сразу
после вставки — пустой список; после `load` + 500 мс та же кнопка находится
(`button>body>html`), а вторая кнопка, вставленная в этот момент, снова не находится
(`body>html` вместо `button>body>html`). То есть ответ отстаёт ровно на мутации,
сделанные после последнего кадра.

## Масштаб

`testdriver.js::click` (и через него `test_driver.bless()`) перед обращением к
исполнителю проверяет `inView` → `getPointerInteractablePaintTree`
(`getClientRects` + `document.elementsFromPoint`) и отклоняется с
`element click intercepted error`, если элемента в дереве нет. `bless()` всегда
вставляет свежую `<button>` и сразу кликает её — поэтому любой WPT, начинающий с
`bless()`, падает на первом шаге. Замерено в `speech-api`: `SpeechSynthesisEvent-properties.html`,
`SpeechSynthesis-speak-events.html` (2 сабтеста), `SpeechSynthesis-speak-twice.html`,
`SpeechSynthesis-pause-resume.tentative.html` — все с `element click intercepted error`;
`docs/tasks/p2-test-track.md` фиксирует ту же строку в `editing` (3 id, «не разбирались»).
Страницы, вызывающие `elementFromPoint` сразу после построения DOM, получают тот же
устаревший ответ.

## Причина (предварительно)

Соседний натив `_lumen_get_client_rects` (`platform.rs`, BUG-1007) первым делом зовёт
`flush.maybe_flush()`, а оба натива хит-теста только читают `hit_test_tree` —
снимок `LayoutBox`, который обновляется вместе с кадром. Нужно проверить, что
`maybe_flush` обновляет и `hit_test_tree` (а не только `layout_rects`/`client_rects`),
и вызвать его в обоих нативах.

Попутное наблюдение: в результате `elementsFromPoint` после `html` идёт ещё один
элемент без `localName` (в `map(e => e.localName).join('>')` — пустой хвост) — вероятно,
узел документа; CSSOM View §3 требует только элементы, `html` последним.
