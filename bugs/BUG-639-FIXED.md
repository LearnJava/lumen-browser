# BUG-639: Navigation API shim is missing several spec-required pieces — `NavigationCurrentEntryChangeEvent`, `updateCurrentEntry()`, `NavigationDestination`, `NavigationHistoryEntry` events

**Статус:** FIXED 2026-09-25 (P3)
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

## Срез 45 WPT-RUN-7 (2026-09-21)

Повторный замер при получении baseline: **307/475 harness OK, 8/430 подтестов** (было 6/417). 283 из 307 harness-`OK` — `Subtests passed 0/1`. Часть
категории не доходит до подтестов вообще из-за [BUG-1075](BUG-1075-OPEN.md) (70 id) — его надо закрыть раньше, чем оценивать остальные пробелы по
числам. Подробности — `docs/tasks/p2-test-track.md#test-3-срез-45-2026-09-21`.

## Исправление (P3, 2026-09-25)

Все четыре дефекта из «Симптома» закрыты в `NAVIGATION_API_SHIM`
(`crates/js/src/navigation_api.rs`), плюс три, найденных по дороге живым
прогоном категории:

1. **`NavigationCurrentEntryChangeEvent`** — класс с `navigationType`/`from`,
   `from` обязателен (`TypeError` и без словаря, и без члена), `navigationType`
   проверяется по enum. `_lumen_fire_currententrychange` диспатчит его (trusted),
   `from` — запись, бывшая текущей до публикации нового состояния, тип
   выводится из смены (`push`/`replace`/`traverse`). В шелле
   `fire_current_entry_change` в двух same-document ветках
   `navigate_back`/`navigate_forward` переставлен **после** `commit_nav_state`:
   иначе событие приходило раньше новых стеков и `from` совпадал с `currentEntry`.
2. **`navigation.updateCurrentEntry({state})`** — `TypeError` без `state`,
   `DataCloneError` на несериализуемом состоянии (до изменений), состояние
   хранится структурным клоном и отдаётся свежим клоном на каждый `getState()`,
   `navigate`-событий нет, `currententrychange` с `navigationType: null`.
   Состояние Navigation API отделено от `history.state` (HTML LS §7.2.6).
3. **`NavigationDestination`** — `url`/`key`/`id`/`index`/`sameDocument`/`getState()`.
   Для push/replace/fragment записи ещё нет (`key`/`id` = `''`, `index` = -1),
   для traversal шелл теперь передаёт ключ целевой записи пятым аргументом
   `_lumen_dispatch_navigate` — назначение описывает её.
4. **`NavigationHistoryEntry extends EventTarget`**, не конструируется со
   страницы, `ondispose`. Объекты записей кэшируются по ключу шелла, поэтому
   `navigation.currentEntry === navigation.currentEntry` (раньше — новый объект
   на каждое чтение). Каждый `_lumen_navigation_set_state` сразу сверяет кэш со
   стеками; запись, выпавшая из них (вытеснение forward-хвоста новой
   навигацией), получает `index = -1` и trusted-событие `dispose`.

Найдено прогоном и исправлено тем же фиксом:

* **Кластер `Cannot read properties of null (reading 'index')`** (48 вхождений,
  «гонка» из раздела выше) — не гонка: шелл публикует стеки только в
  `apply_loaded_page`, то есть **после** исполнения парсерных скриптов, и до
  этого `currentEntry` был `null`. Теперь до первой публикации есть
  временная запись текущего документа; первая публикация перенимает именно этот
  объект (по URL), так что ссылка страницы и выставленное на нём состояние
  сохраняются.
* **`navigate()`/`back()`/`forward()`/`traverseTo()` возвращали `Promise` вместо
  словаря `NavigationResult`** — `navigation.navigate(u).committed` давал
  `undefined`. Теперь `{committed, finished}` синхронно; неизвестный ключ
  `traverseTo` и `back()`/`forward()` без записи — отклонённый результат без
  постановки в очередь; `navigate({state})` с несериализуемым состоянием
  бросает синхронно. Опция `history: 'replace'` читается (раньше — только
  нестандартный `replace: true`).
* `on<event>`-обработчики (`onnavigate`/`onnavigatesuccess`/`onnavigateerror`/
  `oncurrententrychange`, `ondispose`) — IDL-аксессоры прототипа.

Регрессия — `crates/js/src/dom/tests/v8_bug639_navigation_api.rs`, 11 тестов;
на прежнем шиме падают все (фальсификация — подмена `navigation_api.rs` версией
из `main`).

Живой прогон (`run_report.py --all --recursive`, dev-release):
`currententrychange-event` 3/25 → **6/21** подтестов (весь `constructor.html`,
`navigation-updateCurrentEntry.html`, `not-on-load.html`),
`updateCurrentEntry-method` 1/9 → **2/9** (`basic.html`, `no-args.html`).
Четыре same-document теста (`history-back-same-doc`,
`navigation-back-forward-same-doc`, `navigation-navigate[-replace]-same-doc`)
перешли FAIL → ERROR: раньше падали на первом `currentEntry.index`, теперь
доходят до `navigation.navigate('#foo')` и упираются в
[BUG-1075](BUG-1075-OPEN.md) (относительный URL без базы) — не регрессия.

## Остаток (вне этого бага)

* [BUG-1075](BUG-1075-OPEN.md) — `navigate('#frag')` без разбора по базе
  документа; главный блокер категории после этого фикса.
* `history.pushState`/`replaceState`/`location.hash =` не публикуют стеки в JS
  синхронно — шелл узнаёт о них только на следующем проходе
  `about_to_wait`, поэтому `currententrychange` не приходит внутри вызова, как
  требует спека (`history-pushState.html` и соседи: `expected true got false`).
* `iframe.contentWindow.navigation` — только фасад без Navigation API
  ([BUG-480](BUG-480-OPEN.md)).
* Состояние `getState()` живёт в realm документа — cross-document возврат к
  записи его не восстанавливает.
* `NavigateEvent.intercept()`/`preventDefault()` не отклоняют `committed`/
  `finished` результата `navigate()`.
