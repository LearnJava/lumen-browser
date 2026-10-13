# BUG-1440 — `ParentNode.append/prepend/replaceChildren` и `ChildNode.before/after/replaceWith` с `DocumentFragment` ничего не вставляют

**Статус:** OPEN
**Заведён:** 2026-10-08 (P2, WPT-RUN-14 срез 20, `css/selectors` + `css-pseudo` + `css-nesting` + `css-namespaces` + `css-cascade`)
**Область:** js (`crates/js/src/shim/web_api_shim_mid.js` — `append` `:8772`, `prepend` `:8755`, `before` `:8694`, `after` `:8709`, `replaceWith` `:8729`, `replaceChildren` `:8851`; у `DocumentFragment` — `:3736-3758`)

## Симптом

`el.append(fragment)`, `prepend`, `before`, `after` не вставляют детей фрагмента (в `el` попадает сам узел `#document-fragment`, `innerHTML` его не показывает), `replaceWith(fragment)` убирает элемент и ничего не ставит, `replaceChildren(fragment)` очищает родителя. `appendChild(fragment)` и `insertBefore(fragment, null)` работают — там есть ветка `c.__isDocumentFragment__` (`:8639`). Остальные формы (строки, элементы) работают. Побочно: у `DocumentFragment` нет `firstElementChild`, `lastElementChild`, `childElementCount` (`undefined`; `children` есть). `template.content.cloneNode(true)` + `main.append(…)` — шаблон 13 id `css-cascade/scope-*` (129 упавших сабтестов): страница не получает ни `<style>`, ни разметку теста. Тот же приём — на любой странице, которая собирает DOM через фрагмент и `append`.

## Проба

| вызов (`f` — `DocumentFragment` с `<i>` и `<u>`) | результат `main.innerHTML` | ожидается |
|---|---|---|
| `main.append(f)` | `<b id="b"></b>` | `<b id="b"></b><i></i><u></u>` |
| `main.prepend(f)` | `<b id="b"></b>` | `<i></i><u></u><b id="b"></b>` |
| `b.before(f)` / `b.after(f)` | без изменений | вставка рядом |
| `b.replaceWith(f)` | `` (элемент убран) | `<i></i><u></u>` |
| `main.replaceChildren(f)` | `` | `<i></i><u></u>` |
| `main.appendChild(f)` | `<i></i><u></u>` | верно |
| `main.insertBefore(f, null)` | верно | верно |
| `f.firstElementChild`, `f.childElementCount` | `undefined` | `<i>`, `2` |

После `main.replaceChildren(); main.append(f)`: `main.childNodes.length` = 1, `main.firstChild.nodeName` = `#document-fragment` — вставлен сам фрагмент, его дети остаются в нём.

## Как найдено

WPT-RUN-14 срез 20: `css-cascade/scope-implicit.html` и ещё 12 `scope-*` — `main.append(template.content.cloneNode(true))` не добавляет ничего.

## Что делать

Во всех шести методах (и их копиях у `ShadowRoot`/фрагмента) раскрывать `__isDocumentFragment__` так же, как `appendChild`: переносить детей фрагмента по одному, не сам фрагмент. DOM LS §4.2.6 «convert nodes into a node» собирает фрагмент из аргументов — отсюда и единая реализация. Добавить `firstElementChild`/`lastElementChild`/`childElementCount` фрагменту.

## Как проверить

Проба из таблицы; `css/css-cascade/scope-implicit.html`, `scope-nesting.html`.

## Дополнение: WPT-RUN-14 срез 26 (2026-10-09)

У `ShadowRoot` разворачивание `DocumentFragment` не работает ни в одном из методов, включая `appendChild` и `insertBefore(fragment, null)` (у `Element` эти два работают — см. выше): `r.appendChild(tpl.content.cloneNode(true))` кладёт в shadow root узел `#document-fragment`, детей фрагмента там нет. Это отдельная запись — [BUG-1577](BUG-1577-OPEN.md) (вместе с `ShadowRoot.getElementById`, ломающим 27 id `css-shadow`).
