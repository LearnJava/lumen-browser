# BUG-417 — `<template>` в `<head>` не даёт парсеру создать `<body>`: `document.body === null`, страница может отрендериться пустой

**Статус:** OPEN
**Компонент:** html-parser (`crates/engine/html-parser/src/tree_builder.rs:1129-1136` —
`process_template_end_tag`, ветка «simplified reset_insertion_mode»)
**Найден:** 2026-07-28 (P2), WPT-VENDOR-html, при разборе среза `html/syntax` — пробой
`--dump-layout`, не самим прогоном

## Симптом

Страница, у которой `<template>` встречается до появления `<body>` (то есть пока парсер в
режиме «in head»), теряет `<body>` целиком:

```html
<!DOCTYPE html><meta charset=utf-8>
<template id=t><div>inside</div></template>
<p id=b>AFTER-TEXT</p>
<script>console.log('PROBE b=' + b.parentNode.nodeName + ' body=' + document.body);</script>
```

```
PROBE tmpl=yes contentKids=1 contentNames=DIV b=found,parent=HEAD bodyKids=NO-BODY
```

`<p>` оказывается ребёнком **`<head>`**, `document.body` === `null`, и `--dump-layout` даёт
пустое дерево (один корневой Block и `display=none`-скип — ничего не рендерится).

Сам `<template>` при этом разобран верно: `template.content` содержит ровно `DIV`, дерево
шаблона не пострадало. Ломается только то, что идёт **после** него.

## Три случая, разделяющие дефект

| Разметка | `b.parentNode` | `document.body` | Рендерится? |
|---|---|---|---|
| `<template>` первым, `<head>`/`<body>` неявные | `HEAD` | `null` | **нет** |
| `<template>` в явном `<head>`, дальше явный `<body>` | `HTML` | `null` | да |
| `<template>` внутри явного `<body>` | `BODY` | 7 детей | да |

Общее у первых двух: `<template>` увиден в режиме «in head» → `<body>` не создаётся никогда.
Виден ли текст на экране — уже вторичное следствие того, куда именно легло продолжение
(`HEAD` не рендерится, `HTML` рендерится). То есть **`document.body === null` — основной
дефект, а пустая страница — его худший, но не единственный исход**.

Практический вес: `<template>` в `<head>` — обычный приём клиентского шаблонизирования, и
`document.body.appendChild(...)` / `document.body.classList` есть буквально в каждом втором
скрипте; на такой странице всё это падает `TypeError`.

## Первопричина

```rust
// crates/engine/html-parser/src/tree_builder.rs:1129-1136
// Reset insertion mode: if still in nested templates stay InTemplate,
// otherwise fall back to InBody (simplified reset_insertion_mode).
if self.template_mode_stack.is_empty() {
    self.insertion_mode = InsertionMode::InBody;
} else {
    self.insertion_mode = InsertionMode::InTemplate;
}
```

Комментарий честно называет упрощение: вместо спекового «reset the insertion mode
appropriately» (HTML LS §13.2.4.1 — обход стека открытых элементов) парсер безусловно
переходит в `InBody`. Если `</template>` встретился, когда `<body>` ещё не создан, спека
требует вернуться в **`InHead`**; тогда следующий же flow-контент отработает переход
«after head» и **создаст `<body>`**. Прыжок сразу в `InBody` этот переход пропускает: режим
уже «в теле», а самого тела нет, поэтому узлы вставляются в текущий узел (`<head>` или
`<html>`), а `<body>` не появляется никогда.

## Почему это не поймал существующий тест

`tree_builder.rs:3361` `template_in_head` парсит
`<html><head><template><style>body{}</style></template></head><body></body></html>` и
проверяет **только форму самого шаблона** (что он лежит в `<head>`, что его DOM-дети пусты,
что фрагмент непуст). Про `<body>` тест не спрашивает ничего — а именно `<body>` и
теряется. Классический случай «зелёный тест маскирует сломанную фичу»: имя теста обещает
покрытие `<template>` в `<head>`, а утверждения покрывают другое.

## Направление починки

Реализовать `reset_insertion_mode` по спеке (обход `open_elements` сверху вниз с проверкой
`select`/`td`/`tr`/`tbody`/`caption`/`colgroup`/`table`/`template`/`head`/`body`/`frameset`/
`html`) вместо текущей развилки из двух вариантов — это же снимет целый класс родственных
багов, а не только этот. Минимальный вариант, если полный обход пока не по бюджету: при
пустом `template_mode_stack` выбирать `InHead`, если `<body>` ещё не создан, и `InBody`
иначе.

Тест `template_in_head` дополнить утверждениями `document.body` (существует) и «`<p>` после
`</template>` — ребёнок `<body>`, а не `<head>`»; без них починку нечем зафиксировать.
