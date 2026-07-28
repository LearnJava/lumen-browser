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
