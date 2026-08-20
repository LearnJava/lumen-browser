# BUG-413 — `HTMLElement.innerText` / `outerText` отсутствуют целиком (ни геттера, ни сеттера)

**Статус:** OPEN
**Компонент:** js (`crates/js/src/dom.rs` — фабрика живых обёрток `_lumen_build_element`,
`:5516-5880`; рядом уже есть `textContent`)
**Найден:** 2026-07-28 (P2), WPT-VENDOR-html, срез `html/dom`

## Симптом

Проба (`--dump-layout`, обычная страница, элемент `<p id=t1>hello <b>world</b></p>`):

```
el.innerText=undefined | el.outerText=undefined | innerText-in-proto=false
```

Ни на инстансе обёртки, ни на `Element.prototype`. `el.textContent` при этом работает.
Единственное упоминание `innerText` во всём `crates/` — строковая проверка имени свойства в
`crates/js/src/trusted_types.rs:122`, то есть Trusted Types гейтит свойство, которого не
существует.

## Что требует спека

HTML LS §3.2.7 «The `innerText` and `outerText` properties»:

- геттер `innerText` — **rendered text** (в отличие от `textContent`): учитывает
  `display:none`/`visibility`, схлопывание пробелов по `white-space`, `text-transform`,
  переводы строк от блочных боксов и `<br>`; для неотрендеренных (detached) поддеревьев
  падает обратно к `textContent`;
- сеттер `innerText` — заменяет детей, превращая `\n`/`\r`/`\r\n` в `<br>` (в
  `white-space:pre*`-контексте — в текстовые узлы);
- `outerText` — то же, но заменяет **сам элемент**; на detached-узле сеттер обязан бросать
  `NoModificationAllowedError`, присвоение пустой строки удаляет узел и склеивает соседние
  текстовые узлы.

Это единственное место в HTML, где DOM-API зависит от результата layout, поэтому реализация
не сводится к обходу дерева — потребуется доступ к боксам (`lumen-layout`), как у уже
работающих `offsetWidth`/`getComputedStyle`.

## Данные WPT

Срез `html/dom` (`run_report.py --all --root html/dom --recursive`),
`elements/the-innertext-and-outertext-properties/`:

| Файл | Сабтесты |
|---|---|
| `innertext-setter.html` | **0/126** |
| `outertext-setter.html` | **0/43** |

Вместе — 169 сабтестов, около 3,5 % всех сабтестов среза (4 784) и вторая-третья по объёму
позиция в нём. Хелпер обоих файлов строит элемент и сразу читает `offsetWidth` (attached-ветка)
либо `firstChild` (detached-ветка) у результата присваивания, поэтому в логе доминируют
`Cannot read properties of undefined (reading 'offsetWidth')` (63) и
`… (reading 'firstChild')` (63), а у `outerText` — `Cannot set properties of null (setting
'outerText')` (34); первопричина у всех одна — свойства нет.

Геттер-файлы того же каталога (`innertext-getter*.html`) в срезе не выделились отдельным
блоком, но упираются в то же отсутствие.

## Направление починки

Разумно резать на два среза: (1) сеттеры `innerText`/`outerText` — чистая DOM-мутация,
layout не нужен, закрывают ровно эти 169 сабтестов; (2) геттер `innerText` поверх
уже существующего моста в layout. `CAPABILITIES.md` в строке DOM сейчас не заявляет
`innerText`, так что дрейфа документации здесь нет (в отличие от `innerHTML` /
[BUG-368](BUG-368-OPEN.md)).

## Срез 1 влит 2026-08-21 (P3) — сеттеры `innerText`/`outerText`

Ровно тот срез, который предлагает раздел «Направление починки»: чистая
DOM-мутация, layout не нужен. Живёт в фабрике живых обёрток
(`crates/js/src/dom.rs::_lumen_build_element`, JS-шим, путь установки V8),
рядом с уже существующим `textContent`.

Что сделано:

- `_lumen_rendered_text_nids(input)` — «rendered text fragment» HTML LS §3.2.7
  дословно: прогон кодпоинтов без `\n`/`\r` → Text-узел, затем каждый перевод
  строки → `<br>`, причём пара `\r\n` считается **одним** переводом, а `\n\n` и
  `\r\r` — двумя. Ведущий/замыкающий перевод даёт `<br>` без соседнего Text-узла.
- `set innerText` — заменяет всех детей этим фрагментом. `null` → `''`
  (`[LegacyNullToEmptyString]`), `undefined` → строка `'undefined'`.
- `set outerText` — тот же фрагмент, но заменяет **сам элемент**, после чего
  склеивает текстовые узлы, которые стояли по обе стороны от него
  (`_lumen_merge_with_next_text` — умышленно уже, чем `normalize()`: сливаются
  ровно два соседа, дальние Text-узлы остаются раздельными). Присваивание пустой
  строки всё равно вставляет пустой Text-узел, поэтому элемент исчезает, а соседи
  оказываются слиты. Без родителя — `NoModificationAllowedError`.
- Вне HTML-namespace обоих свойств быть не должно (это члены `HTMLElement`), и
  присваивание обязано вести себя как запись в обычный объект — аксессор из
  литерала обёртки перекрывается data-property (`_lumen_assign_as_expando`).
  Проверка идёт по `namespaceURI`, поэтому работает на `createElementNS` и
  промахивается на SVG/MathML, **разобранных из разметки** — это
  [BUG-685](BUG-685-OPEN.md) (парсер не заводит foreign content), −4 сабтеста в
  `innertext-setter.html` и −2 в `outertext-setter.html`.

6 юнит-тестов в `dom.rs` (`inner_text_setter_*`, `outer_text_setter_*`,
`inner_text_and_outer_text_absent_outside_html_namespace`) фиксируют переводы
строк, замену детей одним новым Text-узлом, склейку ровно двух соседей, удаление
элемента пустой строкой, `NoModificationAllowedError` и поведение на SVG.

**Почему баг остаётся OPEN:** геттер `innerText` (срез 2) не сделан — чтение
обоих свойств по-прежнему даёт `undefined`, ровно как и до правки. Он обязан
отдавать *rendered text* и требует доступа к боксам layout (см. «Что требует
спека» выше); это отдельный срез поверх уже существующего моста в layout, тем же
путём, каким работают `offsetWidth`/`getComputedStyle`.
