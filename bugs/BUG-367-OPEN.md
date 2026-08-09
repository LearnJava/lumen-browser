# BUG-367 — живая обёртка `Element` расходится с WebIDL: нет `localName`/`prefix`, `tagName` апперкейсится в чужом namespace, внутренний `__nid__` торчит наружу перечислимым и записываемым, все ~120 членов лежат на инстансе вместо прототипа

**Статус:** OPEN
**Компонент:** js (`crates/js/src/dom.rs:5516-5880` — объектный литерал `_lumen_build_element`, общая фабрика живых обёрток узлов; `dom.rs:5521` — `__nid__`; `dom.rs:5522-5523` — `tagName`/`nodeName`; `dom.rs:4599-4612` — `_lumen_element_prototype_for`; для сравнения корректный паттерн — `dom.rs:4669` и `_lumen_make_character_data` `dom.rs:4616-4640`)
**Найден:** P2, WPT-VENDOR-fenced-frame (2026-07-28), проба `--dump-layout` вне WPT

## Симптом

Обёртка, которую возвращают `document.getElementById`/`querySelector`/
`createElement`/`firstChild`/… для **любого** узла живого документа, — это один
большой объектный литерал. Из-за этого её наблюдаемая форма отличается от
WebIDL по четырём независимым пунктам. Все строки ниже — фактический вывод
`--dump-layout`-проб (`.tmp/ff-probe.html`, `.tmp/ff-probe2.html`,
`.tmp/ff-probe3.html`), движок — дефолтный V8.

### 1. `localName` и `prefix` отсутствуют вовсе (DOM LS §4.9)

```
live div.localName            = undefined      live div.prefix            = undefined
live fencedframe.localName    = undefined      live fencedframe.prefix    = undefined
document.body.localName       = undefined      document.body.prefix       = undefined
documentElement.localName     = undefined      documentElement.prefix     = undefined
createElement('div').localName= undefined      createElement('div').prefix= undefined
'localName' in Element.prototype = false
```

Соседние `tagName`/`nodeName`/`namespaceURI` при этом есть и корректны. То есть
это не «DOM не реализован», а точечная дыра: `localName` — самый частый способ
получить имя тега в нижнем регистре, и он единственный, который переносим между
HTML и XML/SVG. Ровно тот же раскол, что в [BUG-358](BUG-358-OPEN.md): у
параллельного «виртуального» дерева DOMParser `localName` **есть**
(`crates/js/src/dom_parser.rs:79` — `this.localName = tagName.toLowerCase()`), и
внутренние потребители внутри `dom_parser.rs` (селекторы, `getElementsByTagName`,
поиск `html`/`head`/`body` — строки 450, 542, 549-550, 608, 790, 832) читают
именно его. У живого дерева его нет ни на одном узле.

### 2. `tagName` апперкейсится и для не-HTML namespace

```
createElementNS('http://www.w3.org/2000/svg','rect') -> localName=undefined  tagName=RECT  namespaceURI=http://www.w3.org/2000/svg
```

По DOM LS §4.9 `tagName` = qualified name, и апперкейс применяется **только** к
элементам в HTML-namespace внутри HTML-документа. Для SVG/MathML `tagName`
обязан быть `rect`, а не `RECT`. Комментарий в `dom.rs:4573` фиксирует, что
`_lumen_get_tag_name` «always upper-cased», и на этом же значении построена
таблица `_lumen_html_tag_prototypes` — то есть апперкейс сделан осознанно ради
таблицы прототипов, но вытекает в веб-видимый `tagName`.

### 3. Внутренний хэндл `__nid__` — перечислимое, записываемое, конфигурируемое свойство

```
'__nid__' descriptor on live div = value=9 enum=true writable=true conf=true
JSON.stringify(host)             = {"__nid__":9,"tagName":"DIV","nodeName":"DIV","nodeType":1,"namespaceURI":"http://www.w3.org/1999/xhtml","id":"host","cl…
```

Это (а) фингерпринт-маркер Lumen, видимый первым же ключом в `Object.keys` любого
узла, и (б) записываемая ссылка на внутренний идентификатор узла. Последнее имеет
доказанное последствие: потребители внутри шима читают именно `child.__nid__`
(`dom.rs:4273-4281`, `5734-5737` и далее), поэтому перезапись поля на настоящем
элементе перенаправляет мутацию дерева на **другой** узел:

```
a.__nid__ / b.__nid__            = 10 / 12
host.innerHTML before            = AB          (host = <span id=a>A</span><span id=b>B</span>)
a.__nid__ = b.__nid__            -> 12
dest.appendChild(a)              -> host.innerHTML after = A ; dest.innerHTML = B
```

`appendChild(a)` переместил `b`. Скрипту страницы для этого не нужны привилегии —
достаточно одного присваивания. Тот же класс, что пункт (2) в
[BUG-366](BUG-366-FIXED.md) (`navigator.credentials._get_original`, исправлено), но здесь
утечка не на одном служебном объекте, а на каждом узле документа.

Внутри того же файла есть и правильный паттерн: `dom.rs:4669`
(`_lumen_make_doctype`) объявляет `__nid__` как `{ value: nid, enumerable: false }`.
Расходятся именно живые элемент-обёртки.

### 4. Все члены интерфейса — собственные свойства инстанса, а не операции прототипа

```
Object.getOwnPropertyNames(Element.prototype).length = 3
Element.prototype own names                          = ["constructor","attachInternals","setHTML"]
Node.prototype own names                             = ["constructor","hasChildNodes"]
Object.getOwnPropertyNames(host).length              = 134   (из них enumerable = 120)
Object.keys(iframe).length                           = 127
host own 'getAttribute'                              = true    Element.prototype own 'getAttribute' = false
iframe own/proto для src,srcdoc,name,sandbox,allow,referrerPolicy,loading,width,height,contentWindow,contentDocument
                                                     = own=true proto=false (все 11)
{...host} key count                                  = 120
for-in over div                                      = 125
delete c.getAttribute                                = true ; typeof c.getAttribute после delete = undefined
                                                       (у другого элемента метод остался)
```

Наблюдаемые последствия, помимо провала любого `idlharness`: `Object.keys(el)`,
`for…in`, spread и `JSON.stringify(el)` выдают всю реализацию биндинга (у
настоящего `Element` они пусты / бросают на циклах); `delete el.getAttribute`
ломает метод у одного узла, не затрагивая остальные; каждая обёртка несёт ~120
собственных свойств с замыканиями вместо разделяемого прототипа — прямой
перф/память-налог на страницах с большим числом обёрнутых узлов.

[BUG-322](BUG-322-FIXED.md) уже поставил обёрткам `[[Prototype]]` (поэтому
`instanceof` работает и `Element.prototype` вообще существует), но сами члены на
прототип не переехали — прототипы остались почти пустыми.

### 5. (Задокументированное упрощение, а не дефект) неизвестные теги → `HTMLElement`

```
createElement('fencedframe') ctor / instanceof HTMLUnknownElement = HTMLElement / false
createElement('foo')         ctor / instanceof HTMLUnknownElement = HTMLElement / false
createElement('abcd')        ctor / instanceof HTMLUnknownElement = HTMLElement / false
parsed <foo>                 ctor / instanceof HTMLUnknownElement = HTMLElement / false
typeof HTMLUnknownElement = function ; HTMLUnknownElement.prototype -> HTMLElement.prototype (цепочка верная)
```

По HTML LS §3.1.3 нераспознанный тег обязан получать `HTMLUnknownElement`.
Глобал и его цепочка прототипов уже заведены (`dom.rs:4563`), но
`_lumen_element_prototype_for` (`dom.rs:4609-4611`) для тега без записи в
таблице возвращает `HTMLElement.prototype`. Это **прямо задокументировано** как
упрощение в `dom.rs:4576-4578`, поэтому в отдельный баг не выделяется — но
фиксится в том же месте, что пункты 1-2, и без него `<fencedframe>` (и любой
другой не-реализованный элемент) неотличим от валидного HTML-элемента.

## Причина

`_lumen_build_element` (`dom.rs:5516`) строит обёртку одним объектным литералом
`var _obj = { __nid__: nid, get tagName() {…}, …, getAttribute: function(){…}, … }`
и в хвосте функции один раз выставляет ей `[[Prototype]]` (фикс BUG-322). Литерал —
это ~120 собственных перечислимых свойств, среди них и служебный `__nid__`, и все
методы. Прототипы (`Element.prototype`, `Node.prototype`) при этом остались
почти пустыми: на них попало только то, что добавляли отдельными коммитами
(`hasChildNodes` — BUG-327, `attachInternals`, `setHTML`).

`localName`/`prefix` в этом литерале просто не объявлены; `tagName` возвращает
`_lumen_get_tag_name(nid)` — нативную апперкейс-строку — без разбора namespace.

## Масштаб

- **Не только 🚫-скоуп.** Обёртка одна на весь живой DOM, то есть пункты 1-4
  задевают каждую вендоренную и будущую WPT-категорию, а не `fenced-frame`.
  Профильная категория для пунктов 1-2 — уже вендоренная `dom/nodes`
  (`Element-tagName.html`, `Node-properties.html` и др.).
- Пункт 4 — причина, по которой любой `idlharness.*` в принципе не может
  пройти по узловым интерфейсам, даже когда до него доходит очередь
  (сейчас они не доходят из-за невендоренных `WebIDLParser.js`/`idlharness.js`).
- Пункт 3 — единственный из пяти с последствием вне соответствия спеке
  (перенаправление мутации дерева перезаписью поля со страницы).

## Что при этом корректно и ломать при фиксе не надо

- `namespaceURI` — верный (`http://www.w3.org/1999/xhtml` для HTML,
  `http://www.w3.org/2000/svg` для SVG-элементов).
- `nodeName` для HTML-элементов — верный (апперкейс здесь по спеке).
- Интернирование обёрток (`_lumen_make_element`/`_lumen_element_wrappers`,
  BUG-291) и цепочка прототипов (BUG-322) работают: `===`-идентичность узлов и
  `instanceof Element`/`HTMLDivElement` держатся.
- `HTMLUnknownElement` уже существует глобалом с правильной цепочкой — нужен
  только фолбэк, а не новый интерфейс.

## Возможный фикс (не реализован в этой сессии)

1. `localName`: геттер в `_obj`, `_lumen_get_tag_name(nid).toLowerCase()` для
   HTML-namespace, иначе нативное имя как есть; `prefix` — `null` (Lumen не
   парсит префиксы) вместо отсутствия свойства.
2. `tagName`: апперкейс применять только при
   `namespaceURI === 'http://www.w3.org/1999/xhtml'`; таблицу
   `_lumen_html_tag_prototypes` это не задевает, если ключ по-прежнему брать из
   нативного `_lumen_get_tag_name`.
3. `__nid__`: `Object.defineProperty(_obj, '__nid__', { value: nid, enumerable: false })`
   после литерала (как уже сделано в `_lumen_make_doctype`, `dom.rs:4669`).
   `writable: false` дополнительно закрывает перенаправление мутаций; проверить,
   что никакой внутренний код не переприсваивает поле.
4. Перенос членов на прототип — самый крупный пункт, разумно отдельным срезом:
   методы, не зависящие от `nid`-замыкания, переносятся на `Element.prototype`/
   `Node.prototype` и читают `this.__nid__`; геттеры/сеттеры — через
   `Object.defineProperties` на прототипе. Литерал при этом должен схлопнуться
   до `__nid__` + того, что действительно требует замыкания
   (`_classList`/`_style`/`_returnValue`).
5. Фолбэк `HTMLUnknownElement.prototype` в `_lumen_element_prototype_for` —
   одна строка, но требует списка *известных* HTML-тегов (не только тех ~40, у
   которых есть выделенный интерфейс), иначе `<section>`/`<article>` получат
   `HTMLUnknownElement` вместо `HTMLElement`.

Пункты 1-3 и 5 — точечные и независимые; пункт 4 — переработка фабрики обёрток.
