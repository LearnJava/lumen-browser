# BUG-380 — `LumenTestharnessExecutor` не замечает провалившуюся навигацию и опрашивает `window.__lumen_wpt_results` в неизменившемся контексте: тест, идущий следом за реально исполнившимся, получает результаты предыдущего и падает с `AssertionError` вместо своего настоящего исхода

**Статус:** OPEN
**Компонент:** tests/wpt tooling (`tools/wptrunner/wptrunner/executors/executorlumen.py:110-150` — `_run_testharness`: `await session.browsing_context.navigate(...)` без проверки исхода, затем цикл опроса `RESULTS_GLOBAL` без предварительной очистки глобала; срабатывающая защита — `tools/wptrunner/wptrunner/executors/base.py:104`)
**Найден:** P2, WPT-VENDOR-fledge (2026-07-28), `run_report.py --all --root fledge --recursive`

## Симптом

В категории `fledge` из 183 id ровно два не-HTTPS, и оба реально исполнились.
Каждый из них «отравил» следующий за собой тест:

```
67:34.49 TEST_START: /fledge/tentative/fetch-ad-auction-headers-insecure-context.tentative.http.html
67:35.74 TEST_END: Test OK. Subtests passed 0/1. Unexpected 1

67:35.74 TEST_START: /fledge/tentative/fetch-ad-auction-headers.tentative.https.html
67:35.77 Reload: https://127.0.0.1:None/…/fetch-ad-auction-headers.tentative.https.html
67:35.79 Ошибка загрузки …: invalid url: invalid port: "None"
67:35.79 WARNING Exception in TestExecutor.run:
  …executorlumen.py:107 in do_test → base.py:104
  AssertionError: Got results from /fledge/tentative/fetch-ad-auction-headers-insecure-context.tentative.http.html,
                  expected /fledge/tentative/fetch-ad-auction-headers.tentative.https.html
67:35.80 TEST_END: ERROR, expected OK
```

Второй случай идентичен: после `insecure-context.window.html`
(`Test OK. Subtests passed 1/1`) следующий id
`interest-group-passed-to-generate-bid.https.window.html?41-45` получил
`ERROR` с тем же `AssertionError`.

## Причина

`_run_testharness` (`executorlumen.py:110-150`) делает два допущения, оба
неверные при неудачной навигации:

```python
await session.browsing_context.navigate(context=context, url=url, wait="complete")
…
expression = f"window.{RESULTS_GLOBAL} !== undefined ? window.{RESULTS_GLOBAL} : null"
```

1. **Исход навигации не проверяется.** На `https://127.0.0.1:None/…` движок
   печатает `invalid url: invalid port: "None"` и остаётся на прежней странице;
   исполнитель этого не видит и идёт опрашивать результаты.
2. **`RESULTS_GLOBAL` не очищается между тестами.** Исполнитель по устройству
   переиспользует один browsing context на весь прогон (`after_connect`,
   `context_id` берётся один раз), а `window.__lumen_wpt_results` обнуляется
   только естественным образом — созданием нового документа. Если документ не
   сменился, первый же опрос немедленно возвращает результаты **предыдущего**
   теста.

Дальше срабатывает страховка wptrunner (`base.py:104`,
`assert result_url == test.url`) — она и превращает ситуацию в `ERROR`.

## Влияние

- **Ложных «зелёных» не даёт** — ассерт ловит подмену по URL, поэтому неверно
  атрибутированный PASS невозможен. Это ограничивает серьёзность.
- **Маскирует настоящую причину.** Тест, упавший бы честным TIMEOUT по
  HTTPS-порт-гэпу, вместо этого показывает трейсбек про несовпадение URL — и
  при разборе лога это уводит в сторону (в прогоне `fledge` два таких `ERROR`
  выглядят как отдельный класс отказа, хотя это тот же HTTPS-гэп).
- **Проявляется только там, где что-то реально исполняется**, т.е. ровно в
  категориях с сигналом. Чем лучше движок проходит категорию, тем чаще пара
  «успешный тест → тест с неудачной навигацией» встречается.
- Масштаб в этом прогоне: 2 `ERROR` из 183 id.

## Как чинить

1. Проверять исход `browsing_context.navigate`: если навигация не состоялась
   (исключение BiDi или несовпадение URL текущего контекста с запрошенным) —
   немедленно поднимать `ExecutorException("ERROR", …)` с внятным текстом, не
   входя в цикл опроса.
2. Перед навигацией (или сразу после чтения результата) обнулять глобал:
   `script.evaluate("window.__lumen_wpt_results = undefined")` — это снимает
   зависимость от смены документа и делает опрос корректным даже при
   переиспользовании контекста.
3. Дополнительно можно сверять `window.location.href` в опрашиваемом контексте
   с ожидаемым URL перед тем, как принять результат.

## Заметки

- Пункт 2 — минимальная и достаточная правка; пункт 1 улучшает диагностику.
- Это тот же класс, что BUG-301 (результаты testharness не доезжали из-за
  маршрутизации `testharnessreport.js`) — оба про доставку результатов, а не
  про движок.
- Полный лог прогона: `.tmp/wpt_fledge4.log` (строки с `67:34`–`67:35` и
  `77:42`–`77:44`).
