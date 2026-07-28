# BUG-375 — у `URL` работает ровно один сеттер (`href`): остальные девять (`protocol`/`hostname`/`host`/`port`/`pathname`/`search`/`hash`/`username`/`password`) установлены как пустые функции и молча проглатывают присваивание даже в strict mode; `searchParams` не связан обратно с `href`; `username`/`password` всегда пустые

**Статус:** OPEN
**Компонент:** js (`crates/js/src/dom.rs:10755-10790` — IIFE, объявляющая аксессоры `URL.prototype` через локальный хелпер `prop(key, getter, setter)`; ключевая строка `dom.rs:10759`: `set: setter || function() {}`. Конструктор — `dom.rs:10739-10754`)
**Найден:** P2, WPT-VENDOR-fledge (2026-07-28), проба `--dump-layout` вне WPT (`.tmp/fledge-probe2.html`)

## Симптом

Присваивание любому компоненту `URL`, кроме `href`, не делает ничего и не
сообщает об этом. Вывод пробы (дефолтный V8, страница со строкой `"use strict"`):

```
URLSET.protocol = before="https:"      assigned="ftp:"                  after="https:"      [SILENTLY IGNORED]
URLSET.hostname = before="a.example"   assigned="b.example"             after="a.example"   [SILENTLY IGNORED]
URLSET.host     = before="a.example"   assigned="b.example:99"          after="a.example"   [SILENTLY IGNORED]
URLSET.port     = before=""            assigned="99"                    after=""            [SILENTLY IGNORED]
URLSET.pathname = before="/orig"       assigned="/changed"              after="/orig"       [SILENTLY IGNORED]
URLSET.search   = before="?old=1"      assigned="?uuid=1&dispatch=track" after="?old=1"     [SILENTLY IGNORED]
URLSET.hash     = before="#o"          assigned="#frag"                 after="#o"          [SILENTLY IGNORED]
URLSET.username = before=""            assigned="u"                     after=""            [SILENTLY IGNORED]
URLSET.password = before=""            assigned="pw"                    after=""            [SILENTLY IGNORED]
URLSET.origin   = before="https://a.example" assigned="https://evil.example" after="https://a.example"
URLSET.href (the one real setter) = https://b.example/y?q=2 search=?q=2
```

`origin` по спеке действительно readonly — там пустой сеттер безвреден по
результату, но вреден по диагностике (см. ниже). Остальные девять — обычные
записываемые атрибуты URL Standard §4.5.

Три следствия, каждое подтверждено отдельной строкой пробы:

```
URLSET.descriptor of search        = get=function set=function setBody=function() {}
URLSET.searchParams mutation -> href = href=https://a.example/x?a=1 search=?a=1 sp=a=2&b=3
URLSET.credentials parsed          = username="" password="" host=a.example href=https://user:pw@a.example/x
URLSET.internals leak              = ["_href","_protocol","_hostname","_host","_port","_pathname","_search","_hash","_origin","_sp"]
```

1. **Сеттер существует, поэтому strict mode молчит.** Если бы аксессор был
   getter-only, `"use strict"` дал бы `TypeError: Cannot set property`. Пустая
   функция — худший из вариантов: код теряет данные без единого признака.
2. **`searchParams` — односторонний.** `u.searchParams.set('a','2')` меняет
   объект `URLSearchParams` (`sp=a=2&b=3`), но `_search`/`_href` остаются
   прежними (`?a=1`), т.е. `u.href` и `u.searchParams` расходятся навсегда.
   Обратной связи нет: `_sp` заполняется лениво в геттере (`dom.rs:10774-10777`)
   и сбрасывается только сеттером `href`.
3. **`username`/`password` — константные заглушки** (`dom.rs:10772-10773`,
   `return ''`), хотя учётные данные в разобранном URL присутствуют (`href` их
   сохранил).
4. **Внутренние поля web-видимы**: 10 own-свойств `_href`…`_sp` перечисляемы,
   `for…in` по объекту `URL` выдаёт 24 имени вместо 13 (тот же класс, что
   `__nid__` в BUG-367 и `_token` в BUG-371).

## Причина

`dom.rs:10756-10762`:

```js
function prop(key, getter, setter) {
    Object.defineProperty(URL.prototype, key, {
        get: getter,
        set: setter || function() {},
        enumerable: true, configurable: true
    });
}
```

Хелпер задуман так, что сеттер необязателен, и «отсутствующий сеттер»
реализован как **пустая функция вместо его отсутствия**. Далее сеттер передан
ровно один раз — для `href` (`dom.rs:10763`). Все прочие девять вызовов
`prop(...)` идут с двумя аргументами, т.е. получают заглушку.

Компоненты хранятся как независимые поля `_protocol`/`_host`/… , заполненные
разово в конструкторе из `_lumen_parse_url(resolved)`. Общего механизма
«пересобрать `_href` после изменения компонента» в модуле нет — поэтому
починка не сводится к дописыванию девяти сеттеров: нужна ре-сериализация.

## Влияние

`url.search = …` — не экзотика, а базовый идиом построения URL. В вендоренной
категории `fledge` на нём стоит центральный хелпер `fledge-util.sub.js`
(`createTrackerURL`), через который проходит каждый тест категории:

```
FLEDGEUTIL.createTrackerURL = https://a.example/fledge/tentative/resources/request-tracker.py  <-- query LOST
```

`uuid`/`dispatch` теряются целиком — запрос ушёл бы к трекеру без параметров,
и все 36 файлов категории получили бы ложные результаты, даже будь HTTPS-порт
и `testdriver` на месте.

## Как чинить

1. Ввести приватную ре-сериализацию: одна функция `_lumen_url_reserialize(u)`,
   собирающая `_href` из компонентов, и девять сеттеров, которые пишут
   компонент, зовут её и сбрасывают `_sp`. Разбор/сборку логичнее делать через
   тот же нативный `_lumen_parse_url` (собрать строку → распарсить → разложить),
   чтобы не заводить второй парсер URL в JS.
2. Убрать `set: setter || function() {}` — при отсутствии сеттера свойство
   должно оставаться getter-only, чтобы strict mode давал `TypeError`, а не
   тихую потерю. Это правило стоит применить и к прочим местам шима с тем же
   идиомом (`grep -n "function() {}" crates/js/src/dom.rs`).
3. Связать `searchParams` в обе стороны: `URLSearchParams` должен знать про
   родительский `URL` и после мутации звать ре-сериализацию.
4. `username`/`password` — брать из разбора, а не возвращать `''`.
5. Внутренние поля перенести в неперечисляемые слоты
   (`Object.defineProperty(this,'_href',{enumerable:false,…})` или `WeakMap`).

## Заметки

- Пункты 2, 4, 5 механические. Пункт 1 — основной, пункт 3 зависит от него.
- Смежное, но отдельное: у `location` те же компоненты объявлены как **обычные
  data-свойства** и присваивание им не навигирует — BUG-376.
- Проба и вывод целиком: `.tmp/fledge-probe2.html`, `.tmp/fledge-probe2.log`.
