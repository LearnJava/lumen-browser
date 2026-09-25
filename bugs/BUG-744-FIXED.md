# BUG-744: `<style>`, созданный парсингом `innerHTML`, не становится элементом стилей

**Статус:** FIXED 2026-09-18 (P3)
**Компонент:** js (`crates/js/src/dom.rs` — фрагментный парсер `innerHTML`)
**Найден:** P3 попутно при разборе [BUG-743](BUG-743-FIXED.md), 2026-08-10

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

## Замер 2026-09-09 (P6, попутно с [BUG-982](BUG-982-FIXED.md)) — предпосылка изменилась

До BUG-982 `<style>` из `innerHTML` **вообще не попадал в DOM**: фрагмент
разбирался документным парсером, элемент оседал в `<head>` временного
документа, а `parse_html_fragment` забирал только детей `<body>`. То есть
«каскад его не видит» было следствием отсутствия узла, а не работы каскада.

После правки узел есть — живая проба на `dev-release`:
`host.innerHTML='<style>#t{color:rgb(1,2,3)}</style>'` →
`document.querySelectorAll('style').length === 1` (раньше было 0).

**Что это НЕ доказывает:** подхватывает ли теперь `Lumen::refresh_dynamic_css`
этот блок, не проверено — headless-режим завершался раньше, чем срабатывал
таймер с чтением `getComputedStyle`. Владельцу бага стоит начать с повторной
пробы в живом окне: возможно, дефект уже закрыт целиком, возможно — сузился
до одного шага (пересборка листа), но старая формулировка причины больше не
верна.

## Закрытие 2026-09-18 (P3) — дефект не воспроизводится

Живая проба на `dev-release` (`lumen.exe --dump-layout <page>`, полный
пайплайн: парсинг → скрипты → layout):

```js
var holder = document.createElement('div');
holder.innerHTML = '<style>.c { position: fixed; }</style>';
document.head.appendChild(holder.firstChild);
console.log('STYLE_COUNT=' + document.getElementsByTagName('style').length);
```

`STYLE_COUNT=1` — узел учитывается `getElementsByTagName`. Финальный layout
элемента `.c` показывает `position=fixed` — правило из динамически
вставленного листа применено. Причина: [BUG-743](BUG-743-FIXED.md)'s
`refresh_dynamic_css`/`inline_style_fingerprint`/`extract_style_blocks`
(`crates/shell/src/doc_extract.rs`) обходят дерево документа по тегу
`style`, не по способу создания узла — правка получилась обобщённой и
закрыла этот баг как побочный эффект.

Единственное найденное расхождение: **синхронный** `getComputedStyle()`
сразу после вставки листа (до следующего relayout) отдаёт устаревшее
значение (`static`), а не пересчитанное (`fixed`). Не специфично для
`innerHTML` — идентично воспроизводится и через
`document.createElement('style')` + `appendChild`. Это уже заведённый
отдельно пробел [BUG-493](BUG-493-FIXED.md) (ДОРАБОТКА → CSSOM-4,
«`getComputedStyle()` не форсирует синхронный пересчёт стиля»), не часть
этого бага.

Правок кода не потребовалось.
