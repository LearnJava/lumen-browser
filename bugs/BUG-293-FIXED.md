# BUG-293 — Вкладка, открытая `window.open()`, не может загрузить `file://` URL («unsupported scheme: file»)

**Статус:** FIXED 2026-07-19 (P3)
**Компонент:** shell (`crates/shell/src/main.rs` — дренаж popup-запросов + `navigate_to`)
**Найден:** 2026-07-16, при попытке открыть локальный сэмпл во второй вкладке через `window.open('file:///…')`

## Симптом

Страница вызывает `window.open('file:///D:/RustProjects/lumen-browser/samples/page.html')` —
новая вкладка создаётся, но загрузка падает:

```
Reload: file:///D:/RustProjects/lumen-browser/samples/page.html
Ошибка загрузки file:///D:/RustProjects/lumen-browser/samples/page.html: network error: unsupported scheme: file
```

При этом тот же файл в **первой** вкладке (CLI-аргумент) открывается нормально, и automation-пути
(BiDi/MCP/graphic_tests) принимают полные `file:///` URL.

## Причина

Дренаж `window.open()`-запросов (`main.rs`, «window.open() popup requests») заворачивает URL
как есть: `navigate_to(PageSource::Url(url))`. `PageSource::Url` идёт по сетевому пути, который
поддерживает только http/https (`require_http_scheme`) — `file://` отклоняется. Разбор
`file://` → файловый путь существует в двух других местах и не переиспользован здесь:

- CLI: `PageSource::from_arg` превращает путь/`file://` в `PageSource::File`;
- automation: хелпер разбора `file://`-префикса (`main.rs:517` — используется BiDi/MCP-навигацией,
  включая Windows-нюанс `file:///D:/…` → `D:/…`).

Тот же дефект, вероятно, касается и других JS-инициированных навигаций через сетевой путь
(`location.href = 'file://…'` и т.п.) — проверить при фиксе.

## Repro

1. Страница с `<script>window.open('file:///<абсолютный путь к любому html>')</script>`.
2. `cargo run -p lumen-shell -- <эта страница>` (первая вкладка — файл, работает).
3. Вторая вкладка открывается и показывает «unsupported scheme: file».

## Что нужно для закрытия

В дренаже popup-запросов (и, при подтверждении, в остальных JS-навигациях) прогонять URL через
существующий разбор `file://` (хелпер `main.rs:517` / логика `PageSource::from_arg`), получая
`PageSource::File` для файловых URL. Учесть security-аспект: переход web-страницы (http/https)
на `file://` браузеры запрещают — разрешить как минимум для случая file→file (локальная страница
открывает локальную), решение для web→file зафиксировать осознанно (по умолчанию — блокировать
с внятной ошибкой, а не «unsupported scheme»). Регрессионный тест: сценарий Repro открывает
вторую вкладку с содержимым файла.

## Фикс (2026-07-19, P3)

Новая свободная функция `resolve_js_navigation(url, opener) -> Result<PageSource, String>`
(`crates/shell/src/main.rs`, рядом с `page_source_for_automation_url`):

- **Только `file://`** URL получают спец-обработку — резолвятся в `PageSource::File` через
  уже существующий `page_source_for_automation_url` (тот же разбор `file:///D:/…` → `D:/…`
  для Windows). Всё остальное (http(s), `about:*`, относительные URL, уже разрешённые
  JS-движком в абсолютные) идёт прежним `PageSource::Url`-путём **без изменений** — это
  сознательно узкий фикс, чтобы не превратить произвольный не-file URL в путь на диске
  (fallback `page_source_for_automation_url` трактует любой не-http/не-file как `File`).
- **Security web→file:** если `opener` — http/https `PageSource::Url`, переход на `file://`
  возвращает `Err(reason)`; вызывающая сторона печатает внятный диагностик вместо загрузки.
  `file→file` (локальная страница открывает локальную) и не-web-openers (`about:blank`/`Empty`)
  разрешены.

Применён в двух точках `poll_*`-дренажа:

1. **Дренаж popup-запросов** (`window.open()`): opener-схема читается из `self.source`
   **до** `open_new_tab()` (который сбрасывает `self.source` на blank); пустой URL → `about:blank`.
2. **Дренаж `location.href=`/`location.replace()`** (`JsNavigateRequest::Push/Replace`): тот же
   резолвер с `self.source` как opener — раньше эти пути тоже слепо заворачивали URL в
   `PageSource::Url` и ломались на `file://`.

Пути automation-навигации (BiDi/MCP, `main.rs:10908/10916`) уже использовали
`page_source_for_automation_url` и не затронуты.

**Тесты:** 5 юнит-тестов в `mod tests` (`resolve_js_nav_*`): file→file грузит с диска,
Windows drive-slash strip, web→file блокируется, http URL нетронут, file от `about:blank` разрешён.
