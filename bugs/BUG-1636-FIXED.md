# BUG-1636 — `data:`-URL из командной строки открывается как путь к файлу

**Статус:** FIXED 2026-10-18 (P6)
**Заведён:** 2026-10-18 (Chat, при разборе [BUG-1239](BUG-1239-OPEN.md))
**Область:** shell (`PageSource::from_arg`, вызовы — `crates/shell/src/cli_args.rs:650`, `:662`)

## Симптом

`lumen.exe --maximized "data:text/html,<p>hi</p>"`:

```
Ошибка загрузки data:text/html,<p>hi</p>: Синтаксическая ошибка в имени файла, имени папки или метке тома. (os error 123)
```

Окно показывает страницу ошибки. `data:` в `iframe`/`src` поддержан (`crates/shell/src/frames.rs:120`, `:178`),
но аргумент командной строки, видимо, распознаётся как URL только по `http(s):`/`file:`, а остальное
считается путём.

## Что сделать

1. В `PageSource::from_arg` распознавать `data:` (и проверить `about:`, `blob:` — что для них ожидается).
2. Юнит-тест на `from_arg("data:text/html,<p>hi</p>")`.

**Готово, когда:** команда выше открывает страницу с текстом «hi»; тест зелёный.

## Исправление

`crates/shell/src/page_source.rs`: `is_data_url` (схема регистронезависима); `PageSource::from_arg` и
`page_source_for_automation_url` отдают `data:` как `PageSource::Url` — загрузчик разбирает его в процессе
(`fetch_with_redirect`, DATAURL-1), в сеть ничего не уходит. JS-навигация (`resolve_js_navigation`) и так шла
через `PageSource::url`. Тест — `tests::cli::page_source_from_arg_data_url_is_url_not_path`. Проверено
`lumen.exe --dump-layout "data:text/html,<p>hi</p>"` → текст «hi».

`about:` кроме `about:blank` и `chrome_preview` по-прежнему уходит в «путь» — вне этого бага. `blob:` из
командной строки смысла не имеет (блоб живёт только в контексте страницы).

Замечание к исходному описанию: `data:` в `<iframe src>` НЕ поддержан (`frames.rs:178` отказывает, BUG-480) —
это отдельная тема, здесь не менялась.
