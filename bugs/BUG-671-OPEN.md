# BUG-671 — `window.Selection` interface constructor missing entirely

**Статус:** OPEN
**Компонент:** js (`crates/js/src/dom.rs` — `Selection`/`getSelection()` шим)
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
уже открытые [BUG-368](BUG-368-OPEN.md) `innerHTML`-текстовая заглушка,
[BUG-384](BUG-384-FIXED.md) именованный доступ на `window`, [BUG-346](BUG-346-OPEN.md)
`..`-сегменты в `Url::resolve()`, [BUG-462](BUG-462-OPEN.md) `Node.contains`
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
