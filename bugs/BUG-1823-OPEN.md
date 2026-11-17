# BUG-1823 (OPEN): SW-скрипты, сгенерированные .py-обработчиками WPT с инъекцией `%HEADERS%`, падают в изоляте SW на JSON.parse

**Открыт:** 2026-11-17, срез SW-MODULE (ветка `p1-sw-module`).
**Слой:** js (`_sw_run_lifecycle` fetch + SW-изолят).

## Симптом

Тесты `service-workers/service-worker`, чей SW-скрипт собирает .py-обработчик
подстановкой заголовков запроса (`test-request-mode-worker.py` и одноклассники:
`update-module-request-mode.https.html`, `update-no-cache-request-headers.https.html`,
`worker-client-id.https.html`), после того как SW реально активируется
(срез SW-MODULE: очередь сообщений installing-воркеру + асинхронный `updatefound`),
падают при eval скрипта в SW-изоляте:

```
Runtime("Expected ',' or '}' after property value in JSON at position 73 (line 1 column 74)")
```

Шаблон скрипта: `const headersAsJson = '%HEADERS%'; const headers = JSON.parse(headersAsJson);`
— `.py` вставляет `json.dumps(headers)` в **одинарные** кавычки JS. Если среди
заголовков запроса есть значение с `"` (например, `sec-ch-ua`), экранирование
ломается: `\"` внутри одинарной JS-строки разворачивается в `"`, и JSON.parse
получает невалидный текст.

До среза эти тесты не падали здесь только потому, что не доходили до активации SW
(контроллер не устанавливался, `registration.installing` был null) — т.е. дефект
вскрыт починкой жизненного цикла, а не внесён ею.

## Гипотеза (не проверена до конца)

В upstream-браузерах тест проходит — вероятно, потому что они не шлют `sec-ch-ua`
на fetch главного скрипта SW. Нужно проверить, какие заголовки Lumen добавляет в
fetch скрипта SW (`_sw_run_lifecycle`), и сравнить набор с Chrome; вариант фикса —
фильтровать заголовки или эскейпить значение на стороне теста нельзя (корпус
вендорится немодифицированным).

## Критерий закрытия

Три теста выше перестают быть TIMEOUT; `run_corpus.py --prefixes
service-workers/service-worker` не показывает `JSON at position` в логах изолята SW.
