# BUG-417 — `<template>` в `<head>` не даёт парсеру создать `<body>`: `document.body === null`, страница может отрендериться пустой

**Статус:** FIXED 2026-08-22 (P3)
**Компонент:** html-parser (`crates/engine/html-parser/src/tree_builder.rs` —
`process_template_end_tag` + `reset_insertion_mode` + `mode_in_template`)
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

## Что сделано (2026-08-22, P3)

Дефект оказался **двухслойным** — правки только в `process_template_end_tag` не хватило:
исправленный режим тут же затирался обратно уровнем выше.

1. **`process_template_end_tag`** — развилка «пустой `template_mode_stack` → `InBody`, иначе
   `InTemplate`» заменена вызовом уже существовавшего `reset_insertion_mode()`.
2. **`reset_insertion_mode`** — функция была написана по §13.2.4.1, но без двух шагов, ровно
   тех, которые нужны здесь: шаг 11 (`template` → текущий режим шаблона; у нас
   `template_mode_stack` хранит *контентный* режим, поэтому это `InTemplate`) и шаг 12
   (`head` при `last == false` → `InHead`). Заодно доведены до спеки шаг 3 (`last` — узел
   первый в стеке) и подшаги 4.1–4.8 выбора режима для `<select>`: обход предков теперь идёт
   наружу и останавливается на **первом** решающем предке (`template` → `InSelect`,
   `table` → `InSelectInTable`), а не проверяет `any()` по всему стеку, где `template`
   ошибочно засчитывался как табличный контекст.
3. **`mode_in_template`** — делегируя `</template>` в `InHead`, она затем восстанавливала
   `InTemplate` по признаку «режим остался `InHead`, значит `InHead` нас не переключил». После
   правки (1) это условие стало ложным: `InHead` — и есть правильный ответ для шаблона,
   закрытого в голове. Добавлен ранний `return` для `</template>`: режим уже сброшен,
   второй раз его назначать нельзя.

**Проверка.** A/B по тестам крейта: с возвращённой старой развилкой (1) три теста краснеют,
с новой — зелёные, 399/399 `lumen-html-parser`. Сквозная проба
(`--dump-layout`, `.tmp/bug417.html` из «Симптома») даёт
`PROBE tmpl=yes contentKids=1 b.parent=BODY body=kids:4` вместо прежнего
`b=found,parent=HEAD bodyKids=NO-BODY`, и страница раскладывается (зелёный бокс в дереве).

**Нейтральность display list.** `dump_golden.py` — 12/12 совпадений. Голден-набор не содержит
страниц с `<template>`, поэтому дополнительно сделан A/B двух таких страниц
(`graphic_tests/72-host-slotted.html`, `graphic_tests/1000000-final.html`):
`--dump-layout`/`--dump-display-list` до и после правки идентичны (расходится только порядок
строк «Загружена картинка» — асинхронная загрузка, к дисплей-листу отношения не имеет).
Причина ожидаемая: в обеих страницах `<template>` лежит внутри `<body>`, и обход стека
приводит к тому же `InBody`, что давала старая развилка.

**Тесты.** `template_in_head` дополнен проверкой существования `<body>`; добавлены
`template_before_body_still_creates_body` (случай 1 из таблицы: неявные `head`/`body`),
`template_in_explicit_head_keeps_body` (случай 2) и `nested_template_end_returns_to_outer_template`
(страховка на вложенные шаблоны — путь, который старая развилка обслуживала верно).
