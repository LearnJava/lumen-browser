# BUG-383 — IDL-рефлексия HTML-атрибутов почти отсутствует, а вместе с ней и методы активации: `a.href`, `input.disabled/readOnly/required/maxLength/placeholder`, `select.selectedIndex/options`, `textarea.rows` — `undefined`; `HTMLElement.prototype.click()`, `input.select()`, `form.submit()` не существуют

**Статус:** FIXED 2026-07-29
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
* [BUG-367](BUG-367-FIXED.md) — все члены лежат own-свойствами, прототип пуст.
* [BUG-377](BUG-377-OPEN.md) — без `baseURI` нельзя правильно рефлектировать
  URL-атрибуты.
* [BUG-305](BUG-305-FIXED.md) — точечное добавление `src`: пример того, как
  список рос вручную.
* [BUG-441](BUG-441-FIXED.md) — `element.value` из скрипта не доезжает до
  рендера и сбора формы: та же болезнь у *текущего* значения, а не у рефлексии.
* [BUG-444](BUG-444-OPEN.md) — заведён этой правкой: у checkedness нет
  хранилища, отдельного от content-атрибута.

## Как исправлено (2026-07-29)

Рефлексия записана один раз таблицей `{idl, content-attr, kind, default}` и
ставится циклом одной парой универсальных аксессоров на каждый вид
(`string`/`bool`/`long`/`ulong`/`url`/`enum`) — и не на экземпляр, а на
**прототипы интерфейсов**: новое свойство теперь одна строка таблицы и не стоит
ничего на элемент (заодно снимает часть [BUG-367](BUG-367-FIXED.md) — члены
больше не лежат own-свойствами). Вид `url` резолвится относительно base URL
документа: внутренний `_lumen_document_base_url()` (первый `<base href>`,
разрешённый относительно URL страницы, иначе сам URL страницы) поверх уже
существовавшего `_url_resolve` — то есть ожидание «нельзя, пока открыт
[BUG-377](BUG-377-OPEN.md)» не подтвердилось: публичный `Node.baseURI` для
этого не нужен и остаётся за BUG-377.

`type`/`name`/`src` перестали быть own-свойствами каждого элемента, поэтому
`div.src` снова `undefined`, `button.type` — `submit`, `textarea.type` —
`textarea`, `select.type` — `select-one`. Добавлены 24 недостающих интерфейса
`HTML*Element` (AREA/OPTGROUP/FIELDSET/SOURCE/TRACK/TIME/BASE/OUTPUT/…), иначе
их атрибуты сели бы на общий `HTMLElement.prototype`.

Вторая половина заявки — не рефлексия, и сделана отдельно:

* `HTMLFormControlsCollection`/`HTMLOptionsCollection` поверх того же
  Proxy-механизма, что и `HTMLCollection` (`_lumen_make_nid_collection`
  выделен из `_lumen_make_html_collection`), плюс `Symbol.iterator` и
  `Symbol.toStringTag` — `form.elements` больше не «обычный Array, который
  выглядит рабочим»;
* `form.length`, `select.options/selectedOptions/selectedIndex/length/add/
  remove/item/namedItem`, `<option>`.selected/index/text/label, конструктор
  `Option()`;
* граф связей `form` / `labels` / `label.control`;
* выделение текста: `select()`, `setSelectionRange()`, `setRangeText()`,
  `selectionStart/End/Direction` (для типов, где выделение применимо, иначе
  `null`), `indeterminate`, `files`, `list`, `stepUp/stepDown`,
  `textarea.defaultValue/textLength`;
* `HTMLElement.prototype.click()` с полной последовательностью: pre-click
  переключение checkbox/radio → отменяемый `click` → поведение активации
  (`input`+`change`, сабмит, ресет, переход по `<a href>`, `<summary>`,
  `<label>`), либо откат переключения, если обработчик отменил событие;
  повторный вход защищён (цикл label → control → label);
* `form.reset()` целиком на стороне документа; `form.submit()` /
  `requestSubmit()` уходят в шелл новым `NavigateRequest::SubmitForm`, где тело
  отправки вынесено из обработчика клика в `Lumen::run_form_submission` — то
  есть скриптовая отправка идёт ровно тем же кодом кодирования/enctype/
  навигации, что и нажатие кнопки.

Найденное при проверке: `checked` в Lumen хранится самим content-атрибутом (по
нему шелл рисует и собирает форму), поэтому первая же запись `el.checked = …`
уничтожала значение по умолчанию — `defaultChecked` и `form.reset()` не имели
что восстанавливать. Добавлен снимок `_lumen_default_checked`, снимаемый при
первой записи; настоящий dirty-checkedness — [BUG-444](BUG-444-OPEN.md).

**Гейт:** `crates/driver/tests/cases/idl_reflection.rs` — 13 тестов на
**дефолтном (V8)** движке через `InProcessSession`. Это принципиально:
внутрикрейтовые `dom::tests` гоняются на QuickJS, где отсутствующий атрибут
приходит `undefined`, а не `null` ([BUG-442](BUG-442-FIXED.md)), поэтому неверно
написанная проверка присутствия там зеленеет; и сами свойства живут на
прототипах интерфейсов, которые поднимает только настоящая фабрика элементов.
Дополнительно — живая проба `--dump-layout` по `.tmp/probe-383.html` (70
замеров).
