# BUG-376 — `window.location = 'url'` не навигирует, а заменяет объект `Location` строкой; присваивание `location.pathname`/`search`/`protocol`/`host`/`port`/`hostname` тоже не навигирует, а молча портит объект, оставляя `href` прежним

**Статус:** OPEN
**Компонент:** js (`crates/js/src/dom.rs:7412-7428` — литерал `var location = {…}`: аксессоры только у `href` и `hash`, остальные семь компонентов — data-поля; сам `var` делает `window.location` записываемым data-свойством)
**Найден:** P2, WPT-VENDOR-fledge (2026-07-28), проба `--dump-layout` вне WPT (`.tmp/fledge-probe3.html`)

## Симптом

Вывод пробы (дефолтный V8, обычная локальная страница):

```
LOC3.descriptor pathname = {"kind":"data","writable":true}
LOC3.descriptor search   = {"kind":"data","writable":true}
LOC3.descriptor href     = {"kind":"accessor"}

LOC3.assign to search   = search now="?injected=1" href before=file://…/fledge-probe3.html
                          href after=file://…/fledge-probe3.html  <-- object mutated, URL unchanged, no navigation
LOC3.assign to pathname = pathname now="/hijacked" href=file://…/fledge-probe3.html  <-- no navigation
LOC3.assign to protocol = protocol now="ftp:"      href=file://…/fledge-probe3.html

LOC3.state consistency after mutation =
    href=file://…/fledge-probe3.html | pathname=/hijacked | search=?injected=1 |
    toString()=file://…/fledge-probe3.html

LOC3.window.location = string
    = after assignment typeof window.location = string value="about:blank#probe3" still has .href? undefined
```

Три отдельных дефекта:

1. **`window.location = 'https://…'` — самый частый в вебе способ навигации —
   не навигирует.** `location` объявлен как `var`, т.е. это записываемое
   data-свойство глобала (`writable:true, configurable:false`), а не
   `[LegacyUnforgeable]`-аксессор, как требует HTML LS §7.3.5. Присваивание
   просто **затирает объект `Location` строкой**: после него `window.location`
   — это `"about:blank#probe3"`, `location.href`/`assign`/`reload` больше не
   существуют, и весь последующий скрипт страницы работает со сломанным
   `location`. Восстановить нечем: `configurable:false`.
2. **Присваивание компоненту не навигирует и рассинхронизирует объект.**
   `location.pathname = '/hijacked'` меняет поле, но `href` и `toString()`
   остаются старыми — объект начинает лгать сам про себя. Спека требует, чтобы
   каждый из семи компонентов был сеттером, выполняющим навигацию.
   Работают только `href` (`dom.rs:7414`) и `hash` (`dom.rs:7422`), а также
   методы `assign`/`replace`/`reload`.
3. **`Location` как интерфейс отсутствует.** `window.Location === undefined`,
   прототип объекта — `Object.prototype`, `constructor.name === 'Object'`,
   `Object.prototype.toString.call(location)` даёт `[object Object]` вместо
   `[object Location]`, `Symbol.toStringTag` нет, а члены удаляемы
   (`delete location.assign` → `true`, после чего `location.assign` —
   `undefined`). Тот же класс, что BUG-367 для `Element`.

## Причина

`dom.rs:7412-7428` — объект собран литералом:

```js
var location = {
    get href()    { return _lumen_loc_href; },
    set href(v)   { _lumen_navigate_or_fragment(String(v || ''), false); },
    protocol:  _lumen_loc_parts.protocol,
    hostname:  _lumen_loc_parts.hostname,
    host:      _lumen_loc_parts.host,
    port:      _lumen_loc_parts.port,
    pathname:  _lumen_loc_parts.pathname,
    search:    _lumen_loc_parts.search,
    get hash()    { return _lumen_loc_hash; },
    set hash(v)   { _lumen_set_location_hash(v); },
    origin:    _lumen_loc_parts.origin,
    …
};
```

Компоненты — снимок значений на момент создания, поддерживаемый в актуальном
состоянии функцией `_lumen_location_update` (`dom.rs:7402-7411`), которая пишет
в те же поля. Это работает на чтение и ровно поэтому не работает на запись:
поле, в которое пишет движок, страница тоже может перезаписать.

Механизм `_lumen_navigate_or_fragment` (`dom.rs:7452+`) уже есть и умеет
отличать фрагментную навигацию от полной — не хватает только вызовов из
сеттеров компонентов.

## Влияние

- Навигация присваиванием (`window.location = url`, `location.pathname = …`) —
  повседневный код реального веба, а не край спеки; сейчас он молча не делает
  ничего, а в первом случае ещё и разрушает `location` для остального скрипта.
- В вендоренной категории `fledge` `window.location`/`document.location`
  встречаются 258 и 19 раз соответственно — это самый частый API категории.
- `document.location` вдобавок отсутствует целиком — это уже заведённый
  BUG-358 (там же `document.URL`/`documentURI`).

## Как чинить

1. Заменить `var location = {…}` на неперечисляемое свойство глобала с
   getter/setter: геттер возвращает объект `Location`, сеттер выполняет
   навигацию (`_lumen_navigate_or_fragment`). Свойство должно быть
   `configurable:false, writable:false` в терминах аксессора — это и есть
   `[LegacyUnforgeable]`.
2. Семь компонентов перевести из data-полей в аксессоры: геттер читает из
   `_lumen_loc_parts`, сеттер собирает новый URL и зовёт
   `_lumen_navigate_or_fragment`. Внутренние обновления (`_lumen_location_update`)
   должны писать в backing-переменные напрямую, как это уже сделано для
   `hash`/`_lumen_loc_hash` (`dom.rs:7409` + комментарий `dom.rs:7429-7432`).
3. Завести конструктор-интерфейс `Location` с методами на прототипе и
   `Symbol.toStringTag`.

## Заметки

- Пункт 2 архитектурно идентичен BUG-375 (сеттеры компонентов `URL`) и, вероятно,
  должен чиниться тем же срезом: и там и там нужна ре-сериализация URL из
  компонентов.
- Проба и вывод целиком: `.tmp/fledge-probe3.html`, `.tmp/fledge-probe3.log`;
  дескрипторы `window.location` — в `.tmp/fledge-probe2.log`
  (`LOC2.location assignable? = value=object get=undefined writable=true configurable=false`).
