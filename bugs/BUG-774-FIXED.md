# BUG-774 — `StorageEvent.prototype.initStorageEvent` не делает WebIDL-коэрсию/default-подстановку аргументов

**Статус:** FIXED 2026-08-24
**Компонент:** js (`crates/js/src/dom.rs` — `StorageEvent` и `StorageEvent.prototype.initStorageEvent`, ~строка 637)
**Найден:** P2, WPT-VENDOR-webstorage, 2026-08-18 — `run_report.py --all --root webstorage --recursive`
**Исправлен:** P1, 2026-08-24

## Симптом

Текущая реализация:

```js
StorageEvent.prototype.initStorageEvent = function(type, bubbles, cancelable, key, oldValue, newValue, url, storageArea) {
    this.type = type; this.bubbles = !!bubbles; this.cancelable = !!cancelable;
    this.key = key; this.oldValue = oldValue; this.newValue = newValue;
    this.url = String(url); this.storageArea = storageArea;
};
```

Спека требует WebIDL-коэрсию каждого аргумента по его типу
(`type`/`url` — `DOMString`, коэрсируются через `ToString`; `key`/
`oldValue`/`newValue` — `DOMString?`, коэрсируются через `ToString`, но
**отсутствующий/`undefined` аргумент подставляет дефолт `null`**, а не
проходит `ToString(undefined)`). Текущий код присваивает аргументы
как есть, без какой-либо коэрсии для `type`/`key`/`oldValue`/`newValue`, и
не подставляет дефолты для отсутствующих параметров — параметр, который не
был передан вовсе, остаётся `undefined` вместо `null`.

## Подтверждённые провалы (`tests/wpt/webstorage/event_initstorageevent.window.js`)

- **`initStorageEvent` с 1 аргументом** (`event.initStorageEvent('type')`):
  ожидается `event.key === null` (дефолт для непереданного параметра),
  реально `event.key === undefined` — `this.key = key` присваивает
  буквально `undefined`.
- **С 8 `null`-аргументами**: ожидается `event.type === "null"` (строка,
  `ToString(null)`), реально `event.type === null` (объект) — нет `String()`
  коэрсии для `type`.
- **С 8 `undefined`-аргументами**: ожидается `event.type === "undefined"`
  (явно переданный `undefined` — не то же самое, что «аргумент не передан»,
  коэрсируется в строку), реально `event.type === undefined`.

## Предлагаемый фикс

```js
StorageEvent.prototype.initStorageEvent = function(type, bubbles, cancelable, key, oldValue, newValue, url, storageArea) {
    this.type = String(type);
    this.bubbles = !!bubbles;
    this.cancelable = !!cancelable;
    this.key = (key === undefined) ? null : (key === null ? null : String(key));
    this.oldValue = (oldValue === undefined) ? null : (oldValue === null ? null : String(oldValue));
    this.newValue = (newValue === undefined) ? null : (newValue === null ? null : String(newValue));
    this.url = (url === undefined) ? '' : String(url);
    this.storageArea = (storageArea === undefined) ? null : storageArea;
};
```

(`key`/`oldValue`/`newValue` — `DOMString?`: `null` остаётся `null`,
любое другое значение, включая явный `undefined`, коэрсируется в строку —
спека трактует явно переданный `undefined` как «нет значения» только для
параметров без default-значения в сигнатуре IDL; здесь у всех восьми
параметров есть `= null`/`= ""` default, поэтому `undefined`-аргумент
подставляет тот default, а не коэрсируется в строку `"undefined"` — **важно
свериться с точным текстом IDL перед фиксом**, тестовый файл проверяет оба
режима на разных сабтестах, см. `initStorageEvent with 8 undefined arguments`
против `initStorageEvent with 1 argument`.)

Не расследовано отдельно: тот же класс отсутствующей WebIDL-коэрсии может
присутствовать и в других синтетических конструкторах `dom.rs` — не
проверялось в рамках этой сессии, скоуп ограничен `StorageEvent`.

---

## Как исправлено (P1, 2026-08-24)

### Развилка из «Предлагаемого фикса» — разрешена

Заявка оставляла открытым вопрос, что делать с **явно переданным
`undefined`**, и сама себе противоречила (текст абзаца против кода выше него).
Ответ даёт IDL HTML LS §8.6:

```webidl
undefined initStorageEvent(DOMString type,
                           optional boolean bubbles = false,
                           optional boolean cancelable = false,
                           optional DOMString? key = null,
                           optional DOMString? oldValue = null,
                           optional DOMString? newValue = null,
                           optional USVString url = "",
                           optional Storage? storageArea = null);
```

Правило WebIDL §3.2.20 (*overload resolution* / *optional argument with a
default value*): для **optional**-параметра с default-значением аргумент
`undefined` неотличим от отсутствующего — берётся default. Поэтому:

* `key`/`oldValue`/`newValue` (`DOMString? = null`): `undefined` **и**
  отсутствие → `null`; явный `null` → `null`; всё остальное → `ToString`.
* `url` (`USVString = ""`, **не** nullable): `undefined`/отсутствие → `''`,
  а вот `null` → строка `"null"`. Это единственный параметр, где `null` и
  `undefined` дают разные результаты, — и ровно то, что различают сабтесты
  «8 null» и «8 undefined».
* `type` — **обязательный** `DOMString` без default: не подставляет ничего
  никогда, `undefined` → `"undefined"`, `null` → `"null"`, а полное отсутствие
  аргумента → `TypeError`.

Так что оба режима из заявки верны одновременно — они относятся к разным
параметрам, а не к разным трактовкам одного правила.

### Что сделано

`crates/js/src/dom.rs`, блок `StorageEvent` в `WEB_API_SHIM_HEAD`:

* два хелпера `_lumen_se_nullable_str` (`DOMString? = null`) и
  `_lumen_se_default_str` (`USVString = ""`), принимающие сырое значение
  слота `arguments`, — чтобы «отсутствует» и «явный `undefined`» шли одной
  веткой, как и требует правило default-значения;
* `initStorageEvent` объявлен как `function(type)` и читает остальные
  аргументы из `arguments`: WebIDL считает `length` только по обязательным
  параметрам, а тест проверяет `initStorageEvent.length === 1` отдельным
  ассертом («should be redundant, but .length can be wrong with custom
  bindings»). Вызов без аргументов бросает `TypeError`;
* по DOM LS §2.9 («initialize an event») legacy-инициализатор дополнительно
  сбрасывает `isTrusted`/`target`/`defaultPrevented`/`cancelBubble`/
  `_stopImmediate`, чего старый код не делал вовсе.

### Расширение скоупа: сам конструктор `StorageEvent`

Заявка ограничивала скоуп «`StorageEvent`», называя в компоненте только
`initStorageEvent`, — но соседний конструктор в тех же 15 строках болел тем же
классом дефекта, и его провалы (`event_constructor.window.js`) не были
закреплены ни за одним багом. Починены вместе:

* `new StorageEvent(null, {url: null})` давал `url === ''` вместо `"null"`:
  проверка была `init.url != null`, то есть `null` уходил в ветку default —
  словарный член `USVString url = ""` подставляет default **только** для
  `undefined`;
* `type` шёл через базовый `Event`, у которого коэрсия написана как
  `String(type || '')` и схлопывает `null` **и** `undefined` в пустую строку.
  Переписывать базовый класс (его делят все ~20 событий шима) ради одного
  события неправильно, поэтому `StorageEvent` переприсваивает `this.type`
  после `Event.call` — с пометкой в комментарии, почему именно так;
* `StorageEvent.length` был 2 вместо 1 (объявлены `type, init`), а
  `new StorageEvent()` и вызов `StorageEvent('')` **без** `new` не бросали
  ничего. Теперь оба — `TypeError`.

Чего сознательно **не** сделано: `storageArea` не проверяется на
`instanceof Storage` (WebIDL требовал бы `TypeError` на постороннем объекте).
Ни один тест категории этого не измеряет, а лишняя зависимость шима событий от
глобального `Storage` — реальный риск: этот блок живёт в
`WEB_API_SHIM_HEAD`, и любое будущее переиспользование его в области без
Web Storage сломалось бы на `ReferenceError`. `undefined`/`null` → `null`,
всё остальное проходит как есть.

## Гейт

11 юнит-тестов (`cargo test -p lumen-js --features v8-backend --lib storageevent`),
покрывающие все 11 сабтестов обоих WPT-файлов один в один.

Живой прогон обоих файлов через `run_smoke.py` (бинарь main от 2026-08-23
против бинаря ветки), A/B:

| Файл | Было | Стало |
|---|---|---|
| `webstorage/event_constructor.window.html` | 2/6 | **6/6** |
| `webstorage/event_initstorageevent.window.html` | 1/5 | **5/5** |

`Unexpected results: 8` → `0`, `rc=1` → `rc=0`.

Полный прогон категории тем же бинарём
(`run_report.py --all --root webstorage --recursive`, 8 мин 45 с):
**25/54 harness OK, 1237/1277 сабтестов** против 1229/1277 сразу после закрытия
[BUG-773](BUG-773-FIXED.md) в тот же день. Harness-число не сдвинулось, и это
ожидаемо: оба файла `StorageEvent` и раньше доходили до конца — они падали
сабтестами, а не вставали. Остаток категории — [BUG-480](BUG-480-OPEN.md)
(события между документами) и [BUG-901](BUG-901-OPEN.md) (одиночный суррогат →
`U+FFFD` на границе с нативом).
