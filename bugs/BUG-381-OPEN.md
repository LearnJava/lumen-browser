# BUG-381 — весь focus-API отсутствует в JS: `element.focus()`, `document.activeElement`, `document.hasFocus()`, события `focus`/`blur` — ничего нет, хотя шелл держит настоящее состояние фокуса

**Статус:** OPEN
**Компонент:** js (`crates/js/src/dom.rs` — `WEB_API_SHIM`: ни фабрика элементов
`_lumen_make_element`, ни литерал живого `document` (`dom.rs:6989`), ни билдер
отсоединённого документа `_lumen_build_detached_document` не определяют ни
`focus`/`blur`, ни `activeElement`, ни `hasFocus`; `grep -rn activeElement
crates/ --include=*.rs` даёт единственное совпадение — комментарий на
`dom.rs:8259`), shell (`crates/shell/src/main.rs:8110` — состояние `focused_node`,
которое никуда в JS не отдаётся)
**Найден:** P2, WPT-VENDOR-focus (2026-07-28), `run_report.py --all --root focus
--recursive` + проба `--dump-layout` (`.tmp/probe-focus.html`, `.tmp/probe-focus2.html`)

## Симптом

Проба `--dump-layout` по всей поверхности focus-API:

```
doc.activeElement            = undefined
doc.hasFocus                 = undefined
window.focus                 = undefined
window.blur                  = undefined
el.focus                     = undefined      (el = document.createElement('input'))
el.blur                      = undefined
el.tabIndex                  = undefined
el.autofocus                 = undefined
doc.body.focus               = undefined
doc.documentElement.focus    = undefined
Element.prototype.focus      = undefined
HTMLElement.prototype.focus  = undefined
Document.prototype.hasFocus  = undefined
```

При этом рядом:

```
HTMLElement                  = function
FocusEvent                   = function       ← конструктор события есть
_lumen_request_focus         = function       ← внутренний натив есть
_lumen_request_blur          = function
_lumen_last_focused_nid      = number
```

То есть отсутствует не «геттер сломан», а вся спецификационная поверхность
целиком: методы `HTMLElement.focus()`/`blur()` (HTML LS §6.6.4), атрибут
`Document.activeElement` и метод `Document.hasFocus()` (HTML LS §6.6.3),
`Window.focus()`/`blur()`, IDL-рефлексия `tabIndex`/`autofocus`, а также
диспатч событий `focus`/`blur`/`focusin`/`focusout` — в шелле нет ни одной
строки, отправляющей их в DOM (`grep -n '"focus"\|focusin\|focusout\|"blur"'
crates/shell/src/main.rs` — пусто).

В прогоне WPT это выглядит так (единственные два теста категории, которые
вообще доходят до своего утверждения, а не умирают раньше):

```
FAIL anchor element should remain focused after removing href attribute.
     - promise_test: Unhandled rejection with value: object
       "TypeError: anchor.focus is not a function"
FAIL Element.focus() center in both directions
     - promise_test: Unhandled rejection with value: object
       "TypeError: target.focus is not a function"
```

## Причина

Фокус в Lumen реализован **только как состояние шелла** и никогда не был
выведен наружу:

* `crates/shell/src/main.rs:8110` — поле `focused_node: Option<lumen_dom::NodeId>`;
* оно кормит `lumen_layout::set_interactive_state(hover, focus, active)`
  (`main.rs:8788`, `9918`, `10404`), благодаря чему `:focus` в CSS матчится
  по-настоящему (`layout/src/lib.rs:1989`, `FOCUS_NID`);
* оно же кормит платформенный a11y-мост — `platform_bridge.focused_node_changed`
  (`main.rs:13140`, `17857`), откуда идёт `EVENT_OBJECT_FOCUS`;
* по нему же маршрутизируется клавиатурный ввод и IME (`main.rs:17126`, `18266`,
  `22739`).

Мост в JS существует, но он **приватный и односторонний**: `_lumen_request_focus` /
`_lumen_request_blur` (`dom.rs:528`, `v8_runtime.rs:1020`) и глобал
`_lumen_last_focused_nid` (`dom.rs:13123`, синхронизируется из шелла на
`main.rs:3541`/`17858`) заведены исключительно ради `<dialog>.showModal()` /
`close()` (HTML LS §6.6.3, `dom.rs:5610`, `5623`) — то есть ради восстановления
фокуса при закрытии диалога. Ни один из этих нативов не обёрнут в
спецификационный API.

Отсюда парадокс: страница **может** сфокусировать произвольный узел вызовом
внутреннего `_lumen_request_focus(nid)` (натив — обычное свойство `window`, см.
BUG-378), но не может сделать это стандартным `el.focus()`, и никак не может
узнать, что сфокусировано.

## Влияние

**Вне WPT** — это не «дырка в редком API», а отсутствие базового кирпича
интерактивной страницы:

* `autofocus` на форме не работает (атрибут читается только внутри
  `_lumen_find_autofocus_in` для `<dialog>`);
* переход по форме скриптом (`nextInput.focus()` после ввода), фокус-ловушки
  модальных окон, «фокус на поле поиска по `/`», возврат фокуса после закрытия
  меню — всё это мёртво на любой странице;
* виджеты, читающие `document.activeElement` для решения «клик был внутри
  меня или снаружи» (комбобоксы, дропдауны, датапикеры), не работают;
* обработчики `onfocus`/`onblur` не вызываются никогда — валидация полей
  «по уходу с поля» не срабатывает (независимо от BUG-360, который убивает
  атрибутную форму записи любых обработчиков).

**В WPT** — категория `focus` (39 отобранных id) недостижима целиком, но
численный ценник по ней мал: 28 id умирают раньше, на BUG-359 (относительный URL
в `window.open`) и на отсутствии browsing context у `<iframe>`. Прямо на этот
баг падают 2 из 7 исполнившихся теста; ещё несколько (`focus-double-sync-calls`,
`focus-sync-when-blur`, `nested-focus-within-iframe-focus-event`) не доходят до
`focus()` из-за BUG-384. То есть ценник категории занижен вторичными багами —
`focus` останется 0/39 даже после исправления BUG-359/384, пока не сделан этот.

## Как чинить

Данные и механизм уже есть, не хватает связки. Минимальный набор:

1. Натив-геттер `_lumen_get_active_element() -> u32` поверх
   `Shell.focused_node`, доставляемый в JS тем же способом, что и
   `_lumen_last_focused_nid` (или проще — переиспользовать этот глобал, он уже
   синхронизируется на каждом изменении фокуса).
2. `document.activeElement` — геттер, оборачивающий nid в элемент через
   `_lumen_make_element`; по спеке при отсутствии фокуса возвращает `body`, а не
   `null`.
3. `HTMLElement.prototype.focus(options)` / `blur()` — обёртки над уже
   существующими `_lumen_request_focus` / `_lumen_request_blur`. Опции
   `preventScroll` / `focusVisible` можно на первом срезе игнорировать, но
   scroll-into-view по умолчанию требуется (`focus-centers-element.html`).
4. Проверка фокусируемости (`tabindex`, `disabled`, `inert` — последнее уже есть,
   `layout/src/inert.rs`) и IDL-рефлексия `tabIndex`/`autofocus` (см. BUG-383).
5. Диспатч `focus`/`blur` (не всплывают) и `focusin`/`focusout` (всплывают) из
   той же точки шелла, где сейчас меняется `focused_node`
   (`main.rs:13131-13140`, `17845-17857`) — через `_lumen_dispatch_rich`.
   Порядок по HTML LS: `blur` старому → `focusout` старому → `focus` новому →
   `focusin` новому.
6. `document.hasFocus()` — по флагу активности окна (в шелле уже есть, им
   управляется `document.visibilityState`, `dom.rs:13098`).

Тесты категории после этого станут осмысленными: `anchor-remove-href.html`,
`focus-centers-element.html`, `focus-double-sync-calls.html`,
`focus-sync-when-blur.html`, `scroll-matches-focus.html` не требуют ни iframe,
ни `window.open` (последний дополнительно упирается в BUG-382).

## Связанные

* [BUG-383](BUG-383-OPEN.md) — `tabIndex`/`autofocus` — часть общего провала
  IDL-рефлексии; `click()` отсутствует там же, где `focus()`.
* [BUG-384](BUG-384-OPEN.md) — named access on Window; мешает измерить этот баг
  на трёх тестах категории.
* [BUG-378](BUG-378-OPEN.md) — почему `_lumen_request_focus` вообще виден
  странице.
* [BUG-360](BUG-360-OPEN.md) — `onfocus="…"` не работал бы и после починки
  диспатча, пока не исправлены атрибутные обработчики.
