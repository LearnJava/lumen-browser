# BUG-1768 — `cookies/attributes/domain.sub.html` виснет на первом сабтесте

**Статус:** OPEN
**Заведён:** 2026-10-27 (P2, WPT-HOST-WPT — на хосте `localhost` был ERROR «Domain is malformed!», теперь TIMEOUT)
**Область:** не установлена: раннер (`tools/wptrunner/wptrunner/executors/executorlumen.py`) или движок (testdriver из вложенного кросс-ориджин фрейма) — **сначала проверить**

## Симптом

`cookies/attributes/domain.sub.html` — TIMEOUT, ни один сабтест не завершается. На `localhost` тест сразу давал ERROR «Domain is malformed!» (для `localhost` у теста нет корректного регистрируемого домена), так что дальше этой точки Lumen раньше не доходил.

## Цепочка теста

1. Страница открывает кросс-ориджин popup (`window.open` на `www.{{host}}`) и собирает его тесты через `fetch_tests_from_window`.
2. Popup грузит iframe, ждёт его `load` и `postMessage`.
3. Внутри iframe — `test_driver.set_test_context(top.opener.top)` и `test_driver.delete_all_cookies()`.

## Гипотеза

`executorlumen.py` опрашивает только `contexts[0]` (корневую вкладку): действие testdriver, отправленное из вложенного кросс-ориджин фрейма другого окна через `set_test_context(top.opener.top)`, либо не доходит до корня, либо ответ не возвращается обратно во фрейм — промис `delete_all_cookies()` не разрешается. Не исключены и движковые звенья: `postMessage` через `top.opener.top` между кросс-ориджин окнами, `load` iframe в popup.

## Что делать

1. Свести к минимальному тесту: popup + iframe + `test_driver.delete_all_cookies()` с `set_test_context(top.opener.top)`; отдельно без popup (только iframe), отдельно без testdriver (только `postMessage`).
2. По результату — чинить executor (обход всех контекстов/окон, маршрутизация ответа testdriver в исходный фрейм) или движок; если раннер — задача P2, переоформить.

## Как проверить

- `run_smoke.py` на `/cookies/attributes/domain.sub.html` при `browser_host: web-platform.test` — OK (сабтесты завершаются, PASS/FAIL уже по существу).
