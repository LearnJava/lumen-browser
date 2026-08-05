# BUG-639: Navigation API shim is missing several spec-required pieces — `NavigationCurrentEntryChangeEvent`, `updateCurrentEntry()`, `NavigationDestination`, `NavigationHistoryEntry` events

**Статус:** OPEN
**Компонент:** js (`crates/js/src/navigation_api.rs::NAVIGATION_API_SHIM`)
**Найден:** P2, WPT-VENDOR-navigation-api, 2026-08-05

## Симптом

`navigation-api` (скоуп ⬜, API реально реализован в Lumen) — вендорена и
прогнана целиком (`run_report.py --all --root navigation-api --recursive`,
~21 мин, 475 отобранных id): **349/475 harness OK, но только 6/417
сабтестов passed** — 1.4%, при том что `CAPABILITIES.md:195` заявляет
рабочий `window.navigation` без оговорок ("`NavigateEvent.intercept()` +
`preventDefault()` round-trip working").

Чтение `crates/js/src/navigation_api.rs` подтверждает четыре конкретных
пробела в шиме, независимо от лога (не догадка по тексту ошибки, а прямое
чтение исходника):

1. **`NavigationCurrentEntryChangeEvent` глобал отсутствует целиком.**
   `_lumen_fire_currententrychange` (строка 298) диспатчит голый `new
   Event('currententrychange')` вместо `NavigationCurrentEntryChangeEvent`
   со свойствами `.navigationType`/`.from`. Только `NavigateEvent`
   экспортирован в `globalThis` (строка 304); лог: 6× `NavigationCurrentEntryChangeEvent
   is not defined` — весь каталог `currententrychange-event/` (19 тестовых
   файлов) не может пройти дальше первой проверки типа события.

2. **`navigation.updateCurrentEntry()` отсутствует как метод.** Класс
   `Navigation` (строки 93-223) не объявляет такой метод вовсе — лог: 4×
   `TypeError: navigation.updateCurrentEntry is not a function`. Весь
   каталог `updateCurrentEntry-method/` (10 файлов) фейлится на первом же
   вызове.

3. **`NavigateEvent.destination` — голый `URL`, не `NavigationDestination`.**
   `_lumen_dispatch_navigate` (строка 253) строит `destination` как
   `new URL(url, window.location.href)` напрямую — у объекта нет
   `sameDocument`, `getState()`, `key`, `id`, `index`, требуемых спекой
   (HTML LS §7.8.3 `NavigationDestination`). Любой тест, читающий
   `event.destination.sameDocument` или `.getState()`, получает
   `undefined`/`TypeError` (в логе — часть кластера `Cannot read
   properties of undefined (reading 'entry'/'committed'/'aborted')`).

4. **`NavigationHistoryEntry` не наследует `EventTarget`.** Класс
   (строки 24-40) — обычный ES-класс без `extends EventTarget`, поэтому
   `entry.addEventListener('dispose', ...)` / `entry.ondispose` не
   существуют и событие `dispose` никогда не может быть доставлено —
   весь класс тестов на `dispose` (перечисление истории при
   `history.pushState`/навигации, вытесняющей старые entries) структурно
   недостижим.

Помимо этих четырёх подтверждённых по коду дефектов лог показывает третий
кластер — `Cannot read properties of null (reading 'index')` (48 вхождений)
на одно-документных тестах вроде `navigation-history-entry/index-not-in-
entries.html`, `navigation-methods/disambigaute-back.html`,
`navigation-history-entry/entries-when-inactive.html` — похоже на гонку
между коммитом навигации шеллом и чтением `_lumen_navigation_entries_json()`/
`_lumen_navigation_current_index()` из JS (`navigation.currentEntry`
возвращает `null` в окне, где спека требует валидную entry), но точная
причина не установлена — не хватило времени в рамках вендоринг-сессии на
трассировку shell-стороны (`crates/shell` — где строится
`_lumen_navigation_entries_json`). Отдельная investigative задача.

## Причина

Шим (`NAVIGATION_API_SHIM`, JS-строка, единая для V8) реализует
Navigation API как урезанный Phase-0 набросок: только "happy path"
методы/события, без полного набора spec-типов (`NavigationDestination`,
`NavigationCurrentEntryChangeEvent`) и без `updateCurrentEntry()`/entry-level
events вовсе.

## Масштаб

Затрагивает весь `navigation-api` WPT-каталог (475 id) — доминирующая
причина 411 из 417 непройденных сабтестов. Не единственная причина (см.
`cross-window/*` тесты — отдельный, уже задокументированный класс,
многооконность/iframe browsing context отсутствует, `BUG-480`), но
основная в рамках одного документа.

## Дальше

Fix scope: добавить `NavigationCurrentEntryChangeEvent` класс +
`.navigationType`/`.from`, реализовать `updateCurrentEntry({state})`
(обновляет state текущей entry без навигации, диспатчит
`currententrychange`), обернуть `destination` в `NavigationDestination`
(`sameDocument`/`getState()`/`key`/`id`/`index` вычисляются из целевой
entry, если она уже существует в `_shellEntries`, иначе синтезируются),
и сделать `NavigationHistoryEntry extends EventTarget` с диспатчем
`dispose` при вытеснении entry из стека (`_shellEntries` укорачивается).
Гонка null-`currentEntry`/`index` — отдельная investigate-задача до
фикса (нужно проследить, в какой момент шелл публикует
`_lumen_navigation_entries_json`/`_lumen_navigation_current_index`
относительно диспатча `navigatesuccess`/`currententrychange`).

CAPABILITIES.md:195 требует правки — снять безусловное "working" и
добавить 🟡-оговорку со ссылкой на этот баг (сделано тем же коммитом,
что вендоринг).
