# BUG-377 — `Node.baseURI` не реализован ни для одного типа узла: `document.baseURI`, `element.baseURI`, `textNode.baseURI` — `undefined`, а `'baseURI' in document` — `false`

**Статус:** OPEN
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
