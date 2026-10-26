# BUG-1701 — `iframe.contentWindow.navigation` — объект без методов: `navigate`/`entries`/`reload`/`back` не функции, `currentEntry` — `undefined`

**Статус:** OPEN
**Заведён:** 2026-10-26 (P2, WPT-NAV-SCOUT)
**Область:** js/shell (окно под-документа: установка `window.navigation` во вложенном realm — `crates/js/src/navigation_api.rs`, `crates/js/src/frame_bridge.rs`, `crates/shell/src/frames.rs`)

## Симптом

Страница с `<iframe src="/common/blank.html">` после `load`:

```
typeof i.contentWindow.navigation                       → "object"
typeof i.contentWindow.Navigation                       → "function"
Object.getOwnPropertyNames(Object.getPrototypeOf(i.contentWindow.navigation))
                                                        → только Object.prototype (constructor, hasOwnProperty, …)
Object.keys(i.contentWindow.navigation)                 → _cache, _changeFrom, _changeType, _current, _docKeys, _hasPendingState, _keyCounter, _list, _nextEntryId, _pendingState, _rawSeen, _states, _synced
String(i.contentWindow.navigation.currentEntry)         → "undefined"
```

То есть поля экземпляра `Navigation` есть, а прототип с методами и аксессорами (`navigate`, `entries`, `reload`, `back`, `currentEntry`, …) утерян: у объекта под-документа цепочка прототипов не `Navigation.prototype`. Любой вызов даёт `TypeError: i.contentWindow.navigation.navigate is not a function` / `Cannot read properties of undefined (reading 'key')`.

## Как найдено

WPT-NAV-SCOUT, кластер B (`docs/wpt-vendor-notes/navigation-api.md`): 102 id (все OK-харнесс, 0/106 сабтестов), потолок +102 балла (+0.15 п.п. корпуса); `navigation-methods` 52, `navigation-history-entry` 19, `navigate-event` 9, `state` 8. Самые частые первые сообщения: `navigate is not a function` (57 id), `entries is not a function` (16), `reload is not a function` (9), `Cannot read properties of undefined (reading 'key'/'sameDocument'/'id')` (14).

## Что делать

Найти, где под-документу назначается `window.navigation` (объект собирается без прототипа `Navigation` — вероятно копированием полей между realm), и поставить во вложенном окне полноценный экземпляр `Navigation` со своим списком записей (одна запись `about:blank`/`src` после загрузки; по спеке записи фрейма отделены от родительских). Связанная область: [BUG-480](BUG-480-OPEN.md) (контексты просмотра фреймов), [BUG-1198](BUG-1198-FIXED.md) (`reload()` внутри фрейма).

## Как проверить

`i.contentWindow.navigation.entries().length === 1`, `typeof i.contentWindow.navigation.navigate === "function"`, `i.contentWindow.navigation.currentEntry.index === 0`; `run_corpus.py --prefixes navigation-api/navigation-methods`: ни одного сообщения `… is not a function`.
