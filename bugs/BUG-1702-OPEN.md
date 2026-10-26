# BUG-1702 — `window.open(url, "name")` в именованный фрейм возвращает `null`; у cross-origin/`document.domain` фрейма нет `navigation`

**Статус:** OPEN
**Заведён:** 2026-10-26 (P2, WPT-NAV-SCOUT)
**Область:** js/shell (`window.open` с именем существующего фрейма; окно фрейма с другим origin; `a[target]`, `form[target]` в именованный фрейм)

## Симптом

Тесты `navigation-api/navigate-event/cross-window/*` и родственные открывают навигацию в именованный `<iframe name="windowname">` (через `window.open(url, "windowname")`, `<a target>`, `form.submit()`) и ждут `navigate` у `iframe.contentWindow.navigation`. Наблюдаемые сообщения:

* `Cannot read properties of null (reading 'navigation')` (17 id) — `window.open(url, "windowname")` вернул `null` вместо `WindowProxy` фрейма;
* `Cannot set properties of undefined (setting 'onnavigate')` (9 id) — у фрейма с другим origin (`document.domain`, `www1.`-поддомен) `contentWindow.navigation` — `undefined`;
* `onnavigate must not fire in the source window` (5 id) — событие приходит в окно-источник, а не в целевое: цель навигации (`target`) не учитывается при диспатче `navigate`;
* `Cannot read properties of null (reading 'postMessage')` (4 id, `opaque-origin*`).

Проба: `window.open('about:blank')` возвращает объект, но `typeof p.navigation === "undefined"`.

## Как найдено

WPT-NAV-SCOUT, кластер C (`docs/wpt-vendor-notes/navigation-api.md`): 40 id, потолок +40 баллов (+0.06 п.п. корпуса); `navigate-event` 24, `navigation-methods` 11. Часть тестов после этого упирается в [BUG-1701](BUG-1701-OPEN.md) (методы `navigation` во фрейме).

## Что делать

(1) `window.open(url, name)` с именем существующего фрейма — навигировать фрейм и вернуть его `WindowProxy`. (2) Окно `window.open('about:blank')` — отдать ему `navigation`. (3) `navigate` диспатчить в окне целевого контекста навигации, а не источника. Общая основа — [BUG-480](BUG-480-OPEN.md).

## Как проверить

`navigate-event/cross-window/open-crossdocument-sameorigin.html` проходит; `window.open(u, "n") === document.querySelector("iframe[name=n]").contentWindow`.
