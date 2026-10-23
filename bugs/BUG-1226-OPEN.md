# BUG-1226 — service worker: `importScripts` разрешается от scope, идентичность воркера склеена без `/`

**Статус:** OPEN
**Компонент:** js/shell (регистрация сервис-воркера, `importScripts` в SW-изоляте)
**Найден:** P3, при закрытии [BUG-695](BUG-695-FIXED.md), 2026-09-29

## Симптом

`testharness.js` регистрирует воркер для `*.any.serviceworker.html` как
`navigator.serviceWorker.register("<name>.any.serviceworker.js", {scope: "does/not/exist"})`.
Лог браузера при прогоне `urlpattern`:

```
[sw https://localhost:18443does/not/exist] v8 script eval error: Runtime("importScripts: HTTP 404 for https://localhost:18443/does/not/resources/urlpatterntests.js")
```

Две вещи неверны: идентичность воркера — `https://localhost:18443does/not/exist` (нет `/` между
origin и scope), а `importScripts("resources/urlpatterntests.js")` разрешён от каталога **scope**
(`/does/not/`), хотя спека (HTML LS, `importScripts`) берёт базой URL **скрипта** воркера
(`/urlpattern/`). Скрипт воркера падает целиком, `fetch_tests_from_worker` не получает ни одного
теста, harness ждёт до TIMEOUT.

## Масштаб

Все 7 `urlpattern/*.serviceworker.html` — TIMEOUT 0/0 (остальные 23 файла категории — 2447/2451).
Та же причина ожидается у любой вендоренной категории с `.any.serviceworker.html`.

## Дальше

Проверить, где сервис-воркер получает базовый URL для `importScripts` (`sw_worker.rs`) и как
склеивается идентичность из origin и scope (`register`).

## Дополнение (WPT-ENC-SCOUT, 2026-10-23)

У `encoding` (15 id: 5 в корне и 10 в `streams/`, все `*.serviceworker.html`) картина иная, чем у `urlpattern`: все `importScripts` воркера получают 200, но результата тест всё равно не отдаёт и id уходит в TIMEOUT (0/0). Значит, вторая причина (идентичность `https://localhost:18443does/not/exist` без `/` или момент запуска/активации воркера) независима от разрешения базы `importScripts`; проверять её надо отдельно после исправления базы.
