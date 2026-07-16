# BUG-293 — Вкладка, открытая `window.open()`, не может загрузить `file://` URL («unsupported scheme: file»)

**Статус:** OPEN
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
