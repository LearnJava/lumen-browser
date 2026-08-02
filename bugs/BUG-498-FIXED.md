# BUG-498: wptrunner BiDi executor crashes on every test — `ExecutorBrowser` missing `token`

**Статус:** FIXED 2026-08-02
**Дата:** 2026-08-02
**Компонент:** tooling (`tools/wptrunner/wptrunner/browsers/lumen.py::LumenBrowser.executor_browser`)
**Найден:** WPT-RUN-3 срез 10 (`ROADMAP.md`) — попытка массового прогона `css/css-variables`

## Механизм

DEVX-15 (ADR-024 §Access model, влита 2026-08-02, `crates/`-часть коммитом
`5922237e8`) сделала токен на `--bidi-port` обязательным и обновила
`tools/wptrunner/wptrunner/executors/executorlumen.py::LumenBidiTestharnessProtocol.connect`
так, что она читает `self.browser.token` и подмешивает его в
`capabilities.alwaysMatch.token` при `session.new`. `self.browser` в
исполнителе — не сам `LumenBrowser`, а лёгкий `ExecutorBrowser`
(`tools/wptrunner/wptrunner/browsers/base.py`), поля которого берутся из
kwargs-словаря, возвращаемого `LumenBrowser.executor_browser()`. Этот метод
(`browsers/lumen.py:171-172`) обновлён не был — возвращает только
`{"bidi_url": self.url}`, без `token`, хотя свойство `LumenBrowser.token`
(перехват строки `[bidi] token: …` из stderr) в том же коммите уже
добавлено и работает.

## Симптом

Любой прогон wptrunner через BiDi-исполнитель Lumen падает на **каждом**
тесте:

```
AttributeError: 'ExecutorBrowser' object has no attribute 'token'
```

`session.new` никогда не отправляется, воркер уходит в `Max restarts
exceeded`, весь прогон завершается `0/0 harness OK` — "No tests ran". Не
специфично для `css-variables`: блокирует весь трек WPT-RUN (P2) целиком, у
любой категории, задетой после мержа DEVX-15 в `main`.

## Масштаб находки

100% BiDi-прогонов, начиная с коммита `5922237e8` (DEVX-15 merge) в
истории `main`. Обнаружено при первой же попытке массового прогона после
этого мержа (WPT-RUN-3 срез 10).

## Фикс

`executor_browser()` дополнен `"token": self.token` — блокирующее чтение
токена (до 10 с, уже реализовано в свойстве `token`) происходит один раз
при старте воркера, до входа в `connect()`. Заодно подтверждает следствие
для дальнейших WPT-сессий: бинарник обязан быть собран **после**
`5922237e8` — старый бинарник не печатает строку `[bidi] token: …` вовсе,
и `LumenBrowser.token` уйдёт в `RuntimeError` после 10-секундного таймаута
вместо мгновенного `AttributeError`.
