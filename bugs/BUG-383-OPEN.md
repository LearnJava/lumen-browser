# BUG-383 — IDL-рефлексия HTML-атрибутов почти отсутствует, а вместе с ней и методы активации: `a.href`, `input.disabled/readOnly/required/maxLength/placeholder`, `select.selectedIndex/options`, `textarea.rows` — `undefined`; `HTMLElement.prototype.click()`, `input.select()`, `form.submit()` не существуют

**Статус:** OPEN
**Компонент:** js (`crates/js/src/dom.rs` — фабрика живых элементов
`_lumen_make_element`: набор рефлектируемых свойств подобран вручную и
покрывает единицы атрибутов; `Element.prototype` при этом содержит одно имя —
`constructor`, все ~134 члена лежат own-свойствами на каждом экземпляре, см.
BUG-367)
**Найден:** P2, WPT-VENDOR-focus (2026-07-28), проба `--dump-layout`
(`.tmp/probe-forms.html`, `.tmp/probe-click.html`)

## Симптом

Страница с полностью размеченной формой
(`<input type=text value=v name=n maxlength=5 placeholder=p required disabled readonly>`,
`<input type=checkbox checked>`, `<select>`, `<textarea>`, `<form>`, `<a href="#x">`).
`typeof` каждого члена:

```
input.value=string      input.name=string        input.type=string
input.defaultValue=undefined   input.disabled=undefined   input.readOnly=undefined
input.required=undefined       input.maxLength=undefined  input.placeholder=undefined
input.pattern=undefined        input.selectionStart=undefined  input.selectionEnd=undefined
input.select=undefined         input.setSelectionRange=undefined
input.form=undefined           input.labels=undefined
input.tabIndex=undefined       input.autofocus=undefined
input.focus=undefined          input.blur=undefined       input.click=undefined
input.files=undefined          input.step=undefined       input.min=undefined  input.max=undefined
input.validity=object   input.checkValidity=function  input.setCustomValidity=function
input.willValidate=boolean

checkbox.checked=boolean   checkbox.defaultChecked=undefined  checkbox.indeterminate=undefined

select.value=string  select.selectedIndex=undefined  select.options=undefined  select.length=undefined
select.add=undefined  select.remove=function        ← это ChildNode.remove(), не HTMLSelectElement.remove(index)

textarea.value=string  textarea.rows=undefined  textarea.cols=undefined  textarea.select=undefined

form.submit=undefined  form.reset=undefined  form.length=undefined  form.checkValidity=function
form.elements → Object.prototype.toString → "[object Array]"     ← должен быть HTMLFormControlsCollection

a.href=undefined
```

Отдельно про `click()` — его нет нигде, ни на экземпляре, ни в цепочке:

```
div.click                    = undefined
a.click                      = undefined
Element.prototype.click      = undefined
HTMLElement.prototype.click  = undefined
Object.getOwnPropertyNames(Object.getPrototypeOf(div)) = ["constructor"]
Object.getOwnPropertyNames(div).length = 134,  "click" среди них нет
```

## Причина

Живой элемент собирается фабрикой, которая перечисляет рефлектируемые свойства
поимённо, а не выводит их из таблицы «IDL-атрибут ↔ content-атрибут ↔ тип»
(DOM Standard §3.1 / HTML LS §2.6.1). В список попали те, что понадобились
конкретным правкам: `value`, `name`, `type`, `checked`, `src` (добавлен точечно
в [BUG-305](BUG-305-FIXED.md)). Всё остальное — включая `href`, самый частый
рефлектируемый атрибут после `src`, — не добавляли.

Методы активации (`click()`, `select()`, `setSelectionRange()`, `form.submit()`,
`form.reset()`) отсутствуют по той же причине, что и `focus()`/`blur()`
([BUG-381](BUG-381-OPEN.md)): у элемента нет прототипа с методами, а фабрика
раздаёт только то, что перечислено явно.

Валидационная часть, наоборот, сделана (`validity`, `willValidate`,
`checkValidity`, `setCustomValidity`) — то есть это не «формы не реализованы»,
а именно дырявая, набранная вручную поверхность.

## Влияние

* `a.href` — чтение адреса ссылки скриптом (аналитика, роутеры, «открыть в
  новой вкладке», normalize URL) не работает ни на одной странице.
* `el.click()` — стандартный способ программно нажать кнопку/чекбокс и
  единственный способ запустить скачивание через синтетический `<a download>`;
  им же пользуется добрая половина e2e-кода и полифиллов.
* `input.disabled` / `readOnly` / `required` — нельзя ни прочитать, ни
  переключить состояние поля из скрипта: динамические формы (разблокировать
  «Отправить» после согласия) не работают.
* `select.selectedIndex` / `options` — стандартный способ работы с выпадающим
  списком; без него `<select>` управляем только через `value`.
* `input.select()` / `setSelectionRange()` — «выделить всё при фокусе»,
  копирование в буфер, маски ввода.
* `form.submit()` / `reset()` — программная отправка формы.
* `form.elements` как обычный `Array` — тихое расхождение: `.namedItem()` нет,
  доступ по `name` не работает, а `Array`-методы наводят на мысль, что всё в
  порядке.

`CAPABILITIES.md` заявляет DOM «✅ full read/write» — тот же класс дрейфа, что
поймали `innerHTML` ([BUG-368](BUG-368-OPEN.md)) и `fetch`.

## Как чинить

Чинить поштучно — значит воспроизвести ту же ошибку в третий раз. Правильная
форма правки: одна декларативная таблица рефлексии
(`{ tag, idlName, contentName, kind: string|bool|long|url|enum, default }`) и
общий геттер/сеттер поверх `_lumen_get_attr`/`_lumen_set_attr`, из которой
свойства ставятся циклом. `url`-вид (`href`, `src`, `action`, `formAction`,
`cite`, `poster`) должен резолвиться относительно base URL документа — что
сегодня невозможно, пока открыт [BUG-377](BUG-377-OPEN.md) (`Node.baseURI`).
Методы активации (`click`, `select`, `setSelectionRange`, `submit`, `reset`)
логично класть на общий прототип вместе с `focus`/`blur` из BUG-381 — это же
снимет и часть BUG-367 (пустой `Element.prototype`).

## Связанные

* [BUG-381](BUG-381-OPEN.md) — `focus()`/`blur()`/`tabIndex`/`autofocus`
  отсутствуют по той же причине и чинятся в той же точке.
* [BUG-367](BUG-367-OPEN.md) — все члены лежат own-свойствами, прототип пуст.
* [BUG-377](BUG-377-OPEN.md) — без `baseURI` нельзя правильно рефлектировать
  URL-атрибуты.
* [BUG-305](BUG-305-FIXED.md) — точечное добавление `src`: пример того, как
  список рос вручную.
