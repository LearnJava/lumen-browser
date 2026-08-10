# BUG-377 — `Node.baseURI` не реализован ни для одного типа узла: `document.baseURI`, `element.baseURI`, `textNode.baseURI` — `undefined`, а `'baseURI' in document` — `false`

**Статус:** FIXED 2026-08-10 (P3)
**Компонент:** js (`crates/js/src/dom.rs` — ни живой `document` (литерал с `dom.rs:6989`), ни билдер отсоединённого документа `_lumen_build_detached_document` (`dom.rs:4728-4769`), ни фабрики узлов `_lumen_make_node`/`_lumen_build_element`/`_lumen_make_character_data` не определяют `baseURI`. `grep -n baseURI crates/js/src/` по всему workspace даёт ноль совпадений)
**Найден:** P2, WPT-VENDOR-fledge (2026-07-28), проба `--dump-layout` вне WPT (`.tmp/fledge-probe.html`, `.tmp/fledge-probe2.html`)

## Симптом

```
BASE.document.baseURI        = undefined
BASE.body.baseURI            = undefined
BASE.documentElement.baseURI = undefined
BASE.text node baseURI       = undefined
BASE.baseURI in document     = 'baseURI' in document = false
```

Последняя строка отделяет этот баг от «сломанного геттера»: свойства нет вовсе,
ни как own, ни в цепочке прототипов.

`baseURI` — атрибут интерфейса `Node` (DOM Standard §4.4), т.е. должен быть у
каждого узла, а не только у документа, и должен учитывать `<base href>`.
Соответственно отсутствует и вся связанная семантика: элемента `<base>` шим
тоже не обрабатывает.

## Причина

Свойство просто не было реализовано. Смежные атрибуты документа
(`URL`/`documentURI`/`compatMode`/`characterSet`) в билдере отсоединённого
документа есть (`dom.rs:4763-4769`, все — захардкоженные `'about:blank'`), а у
живого документа отсутствуют — это уже заведённый BUG-358. `baseURI` же
отсутствует **везде**, включая билдер, и относится к `Node`, а не к `Document`,
поэтому чинится отдельно: одним геттером на общем прототипе узла, а не
дописыванием ещё одной строки в список атрибутов документа.

Данные для правильного значения в движке есть: `_LUMEN_PAGE_URL` (глобал шима,
заполняемый из Rust) и `_lumen_loc_href`; недостающая часть — учёт
`<base href>` первого элемента `<base>` в дереве, с резолвингом относительно
URL документа.

## Влияние

- `baseURI` — базовый способ для скрипта узнать, относительно чего резолвятся
  ссылки; в WPT-хелперах это первая строка файла. В вендоренной категории
  `fledge` на нём построен `fledge-util.sub.js`:

  ```js
  const BASE_URL = document.baseURI.substring(0, document.baseURI.lastIndexOf('/') + 1);
  ```

  На Lumen это `TypeError: Cannot read properties of undefined (reading 'substring')`
  на третьей строке хелпера, который подключают все 36 файлов категории.
  Проба фиксирует это отдельной строкой: `DOC.baseURI slice (fledge-util line 3)
  = THIS THROWS in fledge-util`.
- Отсутствие обработки `<base href>` — это ещё и расхождение в резолвинге
  ссылок самим движком, если он резолвит относительно URL документа, игнорируя
  `<base>` (проверять отдельно; данный баг фиксирует только JS-сторону).

## Как чинить

1. Добавить геттер `baseURI` на общий прототип узла (там же, где живут
   `nodeType`/`textContent`), чтобы он достался всем типам узлов — элементам,
   тексту, комментариям, документу, фрагментам.
2. Значение: URL документа (`_LUMEN_PAGE_URL` / `location.href`), переопределённый
   `href` первого `<base>` в дереве, отрезолвленный относительно URL документа.
   Для отсоединённых документов — `'about:blank'`, как у соседних атрибутов в
   `_lumen_build_detached_document`.
3. Отдельным пунктом — учёт `<base href>` при резолвинге ссылок на стороне
   движка (проверить `crates/engine/`), если он там не реализован.

## Заметки

- Свойство readonly по спеке — при добавлении не повторять ошибку BUG-375:
  getter-only, без пустого сеттера-заглушки.
- Проба и вывод целиком: `.tmp/fledge-probe.html`/`.log`,
  `.tmp/fledge-probe2.html`/`.log`.

## Что сделано (2026-08-10)

Пункты 1 и 2 плана выполнены; пункт 3 (движок) вынесен в
[BUG-752](BUG-752-OPEN.md) — см. ниже.

**Значение считать было нечем не нужно.** Нужная функция в шиме уже была:
`_lumen_document_base_url()` (`crates/js/src/dom.rs`) — ровно HTML LS §4.2.3
(`href` первого `<base>`, отрезолвленный относительно URL документа, иначе сам
URL документа). Её завели под BUG-383 для IDL-рефлексии вида `url` и явно
пометили комментарием «внутренняя, выставить как `Node.baseURI` — это BUG-377».
Поэтому фикс — не вторая реализация обхода `<base>`, а делегирование в ту же
функцию: разойтись значения `baseURI`, `a.href` и `fetch('rel')` теперь не
могут по построению.

**Один аксессор + четыре own-копии.** Геттер поставлен на `Node.prototype`
(`Object.defineProperty`, `enumerable: true`, `configurable: true`) — этого
хватает всему, что имеет прототипную цепочку: живым обёрткам элементов, текста
и комментариев (`_lumen_build_element` ставит им прототип, BUG-322), отсоединённым
`Text`/`Comment`/`ProcessingInstruction` (`_lumen_make_character_data`, BUG-314),
`DocumentType`, `Attr`.

Отдельная работа была в том, что «узлом» в этом шиме является и то, у чего
`[[Prototype]]` нет вовсе — четыре объектных литерала, до `Node.prototype` не
дотягивающихся. Каждому дана own-копия:

| Литерал | Значение | Почему own |
|---|---|---|
| живой `document` (`var document = {…}`) | `_lumen_document_base_url()` | не instance `Document.prototype` — та же причина, по которой рядом продублированы `hasChildNodes`/`contains` |
| `_lumen_build_detached_document` | `'about:blank'` | документ без browsing context; иначе унаследовал бы базовый URL *живой* страницы, к которой не имеет отношения |
| `_lumen_make_document_fragment` | `_lumen_document_base_url()` | плоский литерал, `return frag` без `setPrototypeOf` |
| `_lumen_make_shadow_root` | `_lumen_document_base_url()` | то же |

Сеттера нет ни у одной из копий (ловушка BUG-375: пустой сеттер принимает
запись, молчит в strict mode и теряет значение; getter-only в sloppy mode
просто игнорирует присваивание, в strict — бросает).

**Гейт:** `dom::tests::v8_bug377_base_uri`, 8 тестов
(`cargo test -p lumen-js --features v8-backend v8_bug377`). Помимо очевидного
они пиннят три вещи, которые легко потерять: `'baseURI' in document` (симптом
из заявки — свойства не было вовсе, а не сломан геттер), ответ всех видов узлов
включая четыре беспрототипных, и `<base href>` (абсолютный и относительный),
переопределяющий `document.URL` — реализация, возвращающая URL документа
безусловно, прошла бы всё остальное и была бы неверной. Своя обвязка
`runtime_at(doc, url)`: соседние `v8_runtime_with_dom` в этом файле ставят
пустой URL страницы, на котором любая проверка `baseURI` выродилась бы в `''`.

**Побочная находка — [BUG-752](BUG-752-OPEN.md)** (пункт 3 плана, проверен и
подтверждён): движок `<base href>` не учитывает вовсе. `ResourceBase::resolve`
(`crates/shell/src/main.rs:4154`) резолвит относительно URL страницы и документа
не видит; `Document::base_href()` (`crates/engine/dom/src/lib.rs:1272`)
реализована по спеке и покрыта пятью юнит-тестами, но продакшн-вызовов у неё
ноль. Итог — на странице с `<base>` скрипт и разметка резолвят один и тот же
относительный URL по разным адресам. Это другой крейт и другая причина, поэтому
отдельным багом, а не расширением этого.
