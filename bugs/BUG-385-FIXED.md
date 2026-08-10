# BUG-385 — Local Font Access реализован по устаревшему черновику: есть `navigator.fonts.query()`, нет `queryLocalFonts()`; `FontData`/`FontAccessManager` конструируются со страницы и пишут поля прямо в получателя

**Статус:** FIXED 2026-08-10
**Компонент:** js (`crates/js/src/local_font_access.rs` — `LOCAL_FONT_ACCESS_SHIM`
целиком; устанавливается из `crates/js/src/lib.rs:1344`)
**Найден:** P2, WPT-VENDOR-font-access (2026-07-28), проба `--dump-layout`
(`.tmp/fa-probe.html`, `.tmp/fa-probe2.html`); все 6 исполняемых тестов
категории в прогон не попали (HTTPS-порт-гэп), поэтому находка целиком из пробы

## Симптом

```
typeof self.queryLocalFonts     = undefined
typeof window.queryLocalFonts   = undefined
typeof navigator.fonts          = object
typeof navigator.fonts.query    = function
navigator.fonts.query() resolved = [] isArray=true
```

Точка входа, которой пользуется вся категория (`font_access_basic`,
`font_access_blob`, `font_access_detached_iframe`, `font_access_permission`,
`font_access_query_select`, `font_access_sorted` — каждый начинается с
`const fonts = await self.queryLocalFonts()`), в движке отсутствует. Вместо неё
установлен `navigator.fonts` — форма из черновика WICG 2020 года, удалённая из
спецификации до её принятия в Chrome 103; ни один браузер её сегодня не
поддерживает и ни один тест WPT её не вызывает.

WebIDL-форма того, что установлено, тоже расходится со спекой:

```
desc(navigator,"fonts")        = {"e":true,"w":true,"c":true,"get":"undefined"}
navigator.fonts own vs proto   = ownFonts=true protoFonts=false
navigator.fonts ctor/tag       = ctor=FontAccessManager tag=[object Object]
new FontData({}) own props     = postscriptName,fullName,family,style
                                 | protoNames=constructor,blob
FontData field writable        = HACKED          (после fd.family = 'HACKED')
FontData tag                   = [object Object]
```

И отдельно — вызов конструктора **как метода получателя** молча пишет в этот
получатель вместо `TypeError`:

```
before: window.postscriptName  = false
window.FontData({family:"LEAK"}) = undefined      ← не бросил
after: hasOwn postscriptName   = true
after: window.family           = LEAK
after: enumerable?             = {"e":true,"w":true,"c":true}
```

То есть одна строка `window.FontData({family:'…'})` заводит на `window` четыре
перечисляемых записываемых глобала `postscriptName`/`fullName`/`family`/`style`.
(При вызове с `this === undefined` — `FontData.call(undefined, {})` — тело всё
же строгое и бросает `TypeError`, так что режим не «sloppy»; проблема в том, что
функция вообще не проверяет `new.target`.)

## Причина

Шим написан как обычный JS-конструктор в стиле ES5:

* `FontData` / `FontAccessManager` — простые функции, `new.target` не проверяется
  (спека: у обоих интерфейсов конструктора **нет**, `new FontData()` обязан
  бросать `TypeError: Illegal constructor`);
* поля `FontData` присваиваются в теле конструктора (`this.family = …`), то есть
  становятся собственными записываемыми перечисляемыми данными, тогда как спека
  требует readonly-геттеров на прототипе;
* `Symbol.toStringTag` не выставлен ни на одном из двух прототипов;
* синглтон ставится присваиванием `navigator.fonts = new FontAccessManager()`
  (`local_font_access.rs:81-83`), а не геттером на `Navigator.prototype`, —
  тот же класс, что [BUG-366](BUG-366-FIXED.md) для `navigator.credentials`;
* точка входа `queryLocalFonts()` не устанавливается вовсе.

Нативных биндингов, о которых говорит doc-комментарий («Phase 1:
`_lumen_local_fonts_query()`»), в рантайме нет:
`_lumen_local_fonts_query=undefined _lumen_local_font_blob=undefined` — то есть
класс [BUG-371](BUG-371-FIXED.md) (перечислимая capability на `window`) здесь
не воспроизводится, обе ветки `if (typeof … === 'function')` мертвы.

## Влияние

* Заглушка не измеряется ни одним тестом апстрима: даже когда HTTPS-порт-гэп
  будет закрыт и 6 тестов категории исполнятся, они упадут на первой же строке
  (`self.queryLocalFonts is not a function`), а не покажут состояние движка.
  Phase 1 из doc-комментария, реализованная как задумано (наполнить
  `navigator.fonts.query()`), не даст **ни одного** зелёного теста.
* Форма `navigator.fonts` — не нейтральный «лишний алиас»: перечисляемое
  собственное свойство `navigator`, которого нет ни в одном браузере, само по
  себе является отпечатком (тот же класс, что
  [BUG-379](BUG-379-FIXED.md)).
* `window.FontData({...})` заводит четыре глобала с крайне общими именами
  (`family`, `style`, `fullName`, `postscriptName`); `window.style` после этого
  затеняет то, что страница ожидала бы там увидеть, а любая проверка вида
  `if (window.family)` на посторонней странице начинает врать.
* Записываемые поля `FontData` ломают утверждение спеки о readonly и позволяют
  подменить `postscriptName` уже полученного дескриптора.

## Как чинить

1. Установить `window.queryLocalFonts(options)` → `Promise<FontData[]>` как
   единственную точку входа (WICG Local Font Access §2), сохранив
   `navigator.fonts` только если для этого есть отдельное решение; по
   умолчанию — удалить, чтобы не отдавать отпечаток.
2. `FontData` — readonly-геттеры `postscriptName`/`fullName`/`family`/`style` на
   прототипе, `Symbol.toStringTag = 'FontData'`, конструктор бросает
   `TypeError` при вызове со страницы (проверка `new.target` не спасает —
   нужен именно запрет, интерфейс не конструируется).
3. То же для `FontAccessManager`, если объект остаётся.
4. Реальное перечисление шрифтов (Phase 1) обязано спрашивать разрешение
   `local-fonts` и требовать transient activation — сейчас разрешение отвечает
   `granted` по умолчанию, см. [BUG-386](BUG-386-OPEN.md); закрывать этот баг,
   не закрыв BUG-386, значит выдать список установленных шрифтов молча.

Регрессия проверяется без WPT: страница, которая делает
`window.FontData && window.FontData({family:'X'})` и утверждает
`!('family' in window)`, плюс `typeof queryLocalFonts === 'function'`.

## Что сделано (2026-08-10)

`LOCAL_FONT_ACCESS_SHIM` переписан целиком по образцу
[BUG-374](BUG-374-FIXED.md) (`FSAL_SHIM`) — ближайшего закрытого дефекта того
же класса.

1. **Точка входа.** `queryLocalFonts(options)` установлена на глобале, поэтому
   доступна и как `self.queryLocalFonts`, и как `window.queryLocalFonts`, и
   голым идентификатором. Она и интерфейсный объект `FontData` определены
   `writable: true, enumerable: false, configurable: true` — по WebIDL ни то,
   ни другое не перечисляется в `for (k in window)`.
2. **Форма черновика 2020 года удалена.** `navigator.fonts` и
   `FontAccessManager` не устанавливаются вовсе: ни один браузер их не
   выставляет и ни один тест не вызывает, так что собственное перечисляемое
   свойство `navigator` было отпечатком и ничем больше (класс BUG-379).
   Отдельного решения сохранять алиас (пункт 1 «Как чинить») не нашлось.
3. **`FontData`.** Конструктора нет: и `new FontData({…})`, и вызов как метода
   получателя (`window.FontData({family:'LEAK'})`) бросают
   `TypeError: Illegal constructor`, поэтому четыре глобала с общими именами на
   `window` больше не заводятся. Четыре атрибута — readonly-геттеры на
   прототипе поверх приватного `WeakMap` (own-properties нет вовсе:
   `Object.keys(fd).length === 0`), геттер на чужом получателе — `Illegal
   invocation`. Выставлен `Symbol.toStringTag = 'FontData'`. Единственный путь
   к дескриптору — `queryLocalFonts()`.
4. **Аргументы по WebIDL.** `QueryOptions` проверяется как словарь (не-объект,
   кроме `undefined`/`null`, — `TypeError`), `postscriptNames` — как
   `sequence<DOMString>` (требуется итерируемость, не «похоже на массив»).
   Операция возвращает промис, поэтому ошибка конвертации отклоняет его, а не
   бросает синхронно.
5. **Гейты §2 — сразу и fail-closed.** Без транзиентной активации —
   `SecurityError` (источник тот же, что у `showOpenFilePicker`:
   `navigator.userActivation`, сейчас захардкожен в `isActive: true`, см.
   [BUG-751](BUG-751-OPEN.md)). Разрешение `local-fonts` спрашивается по
   спековому имени; всё, кроме явного `granted` — включая отсутствующий или
   бросающий Permissions API — это `NotAllowedError`. Гейт не несёт нагрузки,
   пока перечислять нечего; он написан сейчас именно потому, что Phase 1 иначе
   приземлится без него (пункт 4 «Как чинить»).
6. **Перечисления шрифтов ОС по-прежнему нет.** Phase 0 отвечает `[]` — это
   единственный ответ, который ничего не выдаёт, пока
   [BUG-386](BUG-386-OPEN.md) держит `local-fonts` разрешённым по умолчанию.
   Нативные привязки Phase 1 захватываются в замыкание при установке (шим не
   читает `_lumen*` с глобала в момент вызова — иначе страница могла бы
   подменить имя и скормить шиму свой список); когда они появятся, ветка уже
   фильтрует по `postscriptNames`.
7. **Молчаливых подмен нет.** `blob()` без нативной привязки отклоняется
   `NotSupportedError`: пустой `Blob` неотличим от нулевого файла шрифта.
   Нативная привязка, которая бросила, или JSON, который не разобрался,
   отклоняют промис — «шрифтов не установлено» для сломанного моста было бы
   ложью.

**Проверка.** 22 юнит-теста в модуле (`cargo test -p lumen-js --features
v8-backend local_font_access`), включая регрессию из этого файла. Плюс проба
`--dump-layout` на живой странице — по таблице симптомов выше:
`typeof self.queryLocalFonts = function`, `navigator.fonts = undefined`,
`FontAccessManager = undefined`, дескрипторы `{"e":false}`,
`new FontData({})` → `TypeError: Illegal constructor`,
`window.FontData({…})` → `TypeError` и «leaked globals = none»,
`[object FontData]`, `family` — аксессор, `queryLocalFonts()` резолвится
`isArray=true len=0`.

## Связанные

* [BUG-386](BUG-386-OPEN.md) — `permissions.query({name:'local-fonts'})`
  отвечает `granted` по умолчанию; вместе с этим багом определяет, каким
  окажется поведение по умолчанию, когда перечисление шрифтов появится.
* [BUG-361](BUG-361-FIXED.md) — `document.permissionsPolicy.features()` пуст,
  из-за чего падает единственный исполнившийся тест категории.
* [BUG-366](BUG-366-FIXED.md) — тот же класс дефекта формы WebIDL на
  `navigator.credentials`.
* [BUG-379](BUG-379-FIXED.md) — движковые собственные свойства глобала как
  отпечаток.
