# BUG-744: `<style>`, созданный парсингом `innerHTML`, не становится элементом стилей

**Статус:** OPEN
**Компонент:** js (`crates/js/src/dom.rs` — фрагментный парсер `innerHTML`)
**Найден:** P3 попутно при разборе [BUG-743](BUG-743-OPEN.md), 2026-08-10

## Что происходит

`<style>`, полученный разбором `innerHTML` (а не `document.createElement`),
не работает как таблица стилей и не виден перечислению элементов:

```js
var holder = document.createElement('div');
holder.innerHTML = '<style>.c { position: fixed; background: aqua; }<\/style>';
document.head.appendChild(holder.firstChild);   // .c остаётся static
```

То же для контейнера, вставленного целиком (`head.appendChild(holder)`).
`document.getElementsByTagName('style').length` не считает такие узлы: в пробе
`.tmp/b733_style2.html` создаётся 8 листов, перечисляются 6 — ровно те, что
сделаны `createElement`.

Дефект **не** сводится к [BUG-743](BUG-743-OPEN.md) (там лист не попадает в
каскад из-за момента вставки): здесь узел не работает и тогда, когда вставлен
на этапе разбора, то есть до сборки каскада.

## Почему это важно

`innerHTML` со `<style>` внутри — обычная форма вставки критического CSS и
шаблонов компонентов. Тихо: исключения нет, `textContent` у узла читается.

## Как воспроизводить

```
python .tmp/b733_style2.py .tmp/b733_style2.html
```
Случаи `c` и `i` — `static`, остальные — `fixed`.
