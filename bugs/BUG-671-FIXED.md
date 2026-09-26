# BUG-671 — `window.Selection` interface constructor missing entirely

**Статус:** FIXED 2026-09-26 (P3)
**Компонент:** js (`crates/js/src/shim/web_api_shim_mid.js` — синглтон `_lumen_selection`, `Selection`/`getSelection()`)
**Найден:** P2, WPT-VENDOR-selection, 2026-08-06

## Симптом

Категория `selection` (`tests/wpt/selection/`, 100 файлов) — вендорена и прогнана
целиком (`run_report.py --all --root selection --recursive`, ~5:32, 128 отобранных
id): 68/128 harness OK, 22/375 сабтестов.

`window.getSelection()`/`document.getSelection()` работают и возвращают
Selection-подобный объект (методы `collapse`/`extend`/`setBaseAndExtent`/
`addRange` и т.д. присутствуют), но глобальный интерфейс-конструктор
`Selection` не заведён вовсе — не сломанный геттер, свойства нет:

```json
{"hasSelectionInWindow": false, "typeofSelection": "undefined",
 "getSelectionType": "function", "sel": "Object",
 "instOf": "ERR:Right-hand side of 'instanceof' is not an object"}
```

Подтверждено живьём (`--mcp-live-port`, страница вне WPT-раннера): `'Selection'
in window === false`, `window.getSelection().constructor.name === "Object"`
(обычный литерал, не именованный класс), `window.getSelection() instanceof
window.Selection` бросает `TypeError` (RHS не объект).

## Масштаб

`tests/wpt/selection/getSelection.html` — 0/9 сабтестов, каждый начинается с
sanity-check `assert_true("Selection" in window, "…")`, который падает первым,
до проверки реального предмета теста. Тот же sanity-check тиражирован по всей
категории (`shadow-dom/tentative/*`, `textcontrols/*` и др.) — WPT-конвенция
подстраховки от "сломанного интерфейса", но здесь ловит именно этот случай.
Не единственная причина отказов категории (доминирующие независимые классы —
уже открытые [BUG-368](BUG-368-FIXED.md) `innerHTML`-текстовая заглушка,
[BUG-384](BUG-384-FIXED.md) именованный доступ на `window`, [BUG-346](BUG-346-FIXED.md)
`..`-сегменты в `Url::resolve()`, [BUG-462](BUG-462-FIXED.md) `Node.contains`
отсутствует, [BUG-415](BUG-415-FIXED.md) отсоединённый документ без Node-методов/
HTML-аксессоров), но независимая находка, не покрытая ни одним из них.

Вне WPT: любой код, проверяющий тип результата `getSelection()` через
`instanceof Selection` (частый паттерн в редакторских библиотеках и полифиллах),
получит либо `false`, либо исключение вместо ожидаемого `true`.

## Причина

Не установлена (не входит в скоуп WPT-VENDOR-задачи — только вендоринг +
прогон + живая проба). Судя по `constructor.name === "Object"`, объект
`getSelection()` собирается как обычный `{}`-литерал с навешанными методами,
а не через `class Selection {}` + `Object.setPrototypeOf`/`new`, поэтому
глобального имени `Selection` просто негде взяться — тот же класс дефекта,
что уже документирован для `Headers`/`Response` в
[BUG-369](BUG-369-FIXED.md)/[BUG-370](BUG-370-FIXED.md) (ES5-объект вместо
WebIDL-интерфейса), но для `Selection` отдельно не заводился.

## Дальше

Fix scope: завести `class Selection` (или эквивалентный конструктор с верным
`.prototype`) в `crates/js/src/dom.rs`, выставить его на `window`/`globalThis`,
переключить фабрику `getSelection()`/`document.getSelection()` на
`new Selection(...)` вместо литерала. Заодно стоит проверить
`Symbol.toStringTag` (см. класс BUG-369/589) — не проверялось в этой сессии.

## Исправление (2026-09-26, P3)

Причина подтверждена: `_lumen_selection` (`web_api_shim_mid.js`, секция
«Selection interface») собирался литералом `{ get anchorNode() {…}, … }`, и
глобалу `Selection` неоткуда было взяться.

Теперь `function Selection()` бросает `TypeError('Illegal constructor')` (в IDL
нет конструктора), синглтон документа — `Object.create(Selection.prototype)`,
все атрибуты/операции перенесены на `Selection.prototype` (перечислимые,
configurable, геттеры `get <имя>`), каждый член проверяет `this` против
синглтона (`TypeError('Illegal invocation')`), `length` — по IDL
(`setBaseAndExtent` 4, `collapse`/`extend`/`getRangeAt`/… 1). Добавлены
`Symbol.toStringTag = "Selection"`, `globalThis.Selection` и алиас
`setPosition` → `collapse` (§3). Таблица длин — без прототипа: иначе
`lengths.toString` находил `Object.prototype.toString` и ставил функцию в `length`.

Проверка: тесты `dom::tests::v8_bug671_selection_interface` (3). WPT
`selection` (128 id): 68/128 → **116/128 harness OK**, 8585/34970 сабтестов —
sanity-check `"Selection" in window` больше не роняет файлы `addRange-*`/
`collapse-*`/`extend-*`/`selectAllChildren` целиком, они впервые выполняют свои
тысячи сабтестов. Baseline обновлён (`--update-expected`, 47 `.ini` переписано,
10 удалено), контрольный `--check` чистый.

Не регрессии, хотя `--check` их показал до обновления baseline:
- 6 файлов OK→ERROR (`canvas-click`, `user-select-on-input-and-contenteditable`,
  `*/initial-selection-during-focus-event-propagation`) — `test_driver.click`
  строит селектор `*|body > *|div`, отвергаемый движком
  ([BUG-1063](BUG-1063-OPEN.md)); бинарник `main` до фикса даёт тот же ERROR —
  дрейф baseline от 2026-08-06.
- `onselectionchange-on-document.html` FAIL→TIMEOUT — раньше `setPosition`
  отсутствовал и тест падал сразу, теперь дожидается `selectionchange`, которое
  не диспатчится вовсе ([BUG-857](BUG-857-OPEN.md)).

Остаток вне скоупа: атрибут `direction`, заглушки `containsNode`/
`getComposedRanges`/`modify`, `removeRange` не бросает `NotFoundError`.
