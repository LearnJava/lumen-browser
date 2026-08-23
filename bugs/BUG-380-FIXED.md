# BUG-380 — `LumenTestharnessExecutor` не замечает провалившуюся навигацию и опрашивает `window.__lumen_wpt_results` в неизменившемся контексте: тест, идущий следом за реально исполнившимся, получает результаты предыдущего и падает с `AssertionError` вместо своего настоящего исхода

**Статус:** FIXED 2026-08-10
**Компонент:** tests/wpt tooling (`tools/wptrunner/wptrunner/executors/executorlumen.py:110-150` — `_run_testharness`: `await session.browsing_context.navigate(...)` без проверки исхода, затем цикл опроса `RESULTS_GLOBAL` без предварительной очистки глобала; срабатывающая защита — `tools/wptrunner/wptrunner/executors/base.py:104`)
**Найден:** P2, WPT-VENDOR-fledge (2026-07-28), `run_report.py --all --root fledge --recursive`
**Исправлен:** P3 2026-08-10, ветка `p3-bug-380`

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

## Проверка механизма перед починкой (2026-08-10)

Конкретный триггер из заявки (`invalid port: "None"`) к моменту разбора уже был
закрыт — `WPT-RUN-2` (eddd6fa1a, 2026-08-02) прибил `--ssl-type=pregenerated`,
и HTTPS-порт теперь выделяется всегда. Поэтому дефект перепроверялся отдельно,
прямой пробой по BiDi (три независимых способа завалить навигацию: закрытый
порт, несуществующий файл, синтаксически битый URL) — воспроизвёлся во всех
трёх, и проба заодно опровергла пункт 1 из «как чинить»:

- **`browsingContext.navigate` отвечает УСПЕХОМ** (`{navigation, url}`) на
  все три битых URL — исключения нет, проверять нечего. `bc_navigate`
  (`crates/bidi-server/src/protocol.rs`) поднимает ошибку только если
  сорвались сами `LiveWindowSession::navigate`/ожидание `DocumentReady`;
  страница, которая не загрузилась, до BiDi-ответа не доезжает вовсе
  (`LiveWindowSession::navigate` к тому же оптимистично пишет запрошенный URL
  в `current_url` ещё до попытки загрузки).
- **Старый документ остаётся живым целиком**: `location.href` по-прежнему
  указывает на предыдущую страницу, `window.__lumen_wpt_results` держит её
  результат.
- **Успешная навигация действительно даёт новый глобал** — маркер, выставленный
  до неё, исчезает и при переходе на другой URL, и при перезагрузке того же
  (это и делает подмену документа надёжным признаком).

## Фикс

`executorlumen.py::_run_testharness`:

1. **`RESET_EXPRESSION` перед каждой навигацией** — обнуляет `RESULTS_GLOBAL` и
   слот testdriver-действия (`__lumen_td_slot`: чужое недренированное действие
   — та же перекрёстная утечка) и метит уходящий документ глобалом
   `__lumen_wpt_stale`. Присваивание `undefined`, а не `delete`, чтобы не
   зависеть от конфигурируемости свойства. Best-effort: на стартовом
   `about:blank` JS-контекста ещё нет («JS context not available»), и наследовать
   там нечего.
2. **Ветка `k == "s"` в цикле опроса** — если через `NAV_SETTLE_S` (2 с; на
   успешном пути не стоит ничего, потому что маркера уже нет на первом же
   опросе) документ всё ещё отвечает с маркером, поднимается
   `ExecutorException("ERROR", …)` с URL, на котором реально остались. Проверка
   стоит **до** чтения результата: иначе поздний коллбэк async-теста
   предыдущей страницы успел бы заново заполнить `RESULTS_GLOBAL` на той же
   неподменённой странице.
3. **`BidiException` вокруг `navigate`** → `ExecutorException("ERROR", …)`.
   Ловит только тот случай, когда движок навигацию действительно отверг (плохой
   контекст, таймаут ожидания); незагрузившуюся страницу ловит пункт 2.

Признак теперь — по идентичности документа, а не по сверке URL (пункт 3 из
исходного «как чинить»): сверка сломалась бы на серверном редиректе.

## Проверка

- `tests/wpt/verify_bug380_navigation_staleness.py` — гоняет **настоящую**
  корутину `_run_testharness` (не её пересказ) через две навигации: страница,
  которая кладёт корректный результат, затем мёртвый порт. Красный до фикса
  (`a failed navigation returned a result instead of erroring: [...] (this is
  test 1's result)` — ровно симптом заявки), зелёный после.
- `tests/wpt/run_suite.py` (гейт S7, curated dom/nodes): `Ran 61 checks,
  Unexpected results: 0`, 28 с — регрессии и замедления нет.
- `/reporting/generateTestReport.html` + `/reporting/bufferSize.html` —
  путь testdriver-действий (ветка `k == "a"`, которую фикс переставил в `else`)
  жив: 2/2 OK.
- **Живой корпус, на котором дефект и воспроизводился**: категория `web-share`
  (прогон 2026-08-09 зафиксировал ровно три отравления — каждый из трёх
  исполнившихся не-`.https.`-тестов ронял следующий id). Повторный
  `run_report.py --all --root web-share --recursive` после фикса: ни одного
  `Got results from <prev>`, при этом исполнившиеся тесты дают прежние
  **3/15 harness OK, 2/6 сабтестов** — регрессии нет.

## Побочный эффект: TIMEOUT → ERROR на не загружающихся тестах

Тест, чья страница не загрузилась, теперь падает `ERROR` с текстом
`navigate(...) reported success but the document was never replaced (still at
<href>)` примерно за 2 с вместо ~20–30 с `TIMEOUT`. Это заметно при сверке с
записанными прогонами вендор-заметок: 12 `.https.`-тестов `web-share`, стоявших
на TLS-гэпе [BUG-657](BUG-657-OPEN.md), в новом прогоне числятся `ERROR`, а не
`TIMEOUT` (весь прогон — 54 с). Статус хуже читается как «ожидаемый», зато
причина названа прямо, а не спрятана за таймаутом. Исключение — **первый** тест
прогона: на стартовом контексте JS-рантайма ещё нет, маркер поставить некуда,
поэтому он по-прежнему честно таймаутится.

## Заметки

- Это тот же класс, что BUG-301 (результаты testharness не доезжали из-за
  маршрутизации `testharnessreport.js`) — оба про доставку результатов, а не
  про движок.
- Полный лог прогона: `.tmp/wpt_fledge4.log` (строки с `67:34`–`67:35` и
  `77:42`–`77:44`).
- Осталось незакрытым (движок, не тулинг): навигация на недостижимый URL
  молча оставляет прежнюю страницу и рапортует успех — ни BiDi-ошибки, ни
  страницы ошибки. Это [BUG-438](BUG-438-FIXED.md), и проба этого разбора
  расширила его скоуп: дефект **не специфичен для `data:`**, он одинаково
  воспроизводится на закрытом порту, несуществующем файле и синтаксически
  битом URL. Новый баг не заводился — дополнена заявка BUG-438.
