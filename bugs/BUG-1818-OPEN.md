# BUG-1818 — 12 файлов `a-tag` в `referrer-policy/4K` — TIMEOUT без единого сабтеста

**Статус:** OPEN, не локализовано.
**Тип:** не установлен (зависание либо непройденная навигация `a`-тега в именованный iframe).
**Заведён:** 2026-11-15 (P3, срез 1 [BUG-1235](BUG-1235-OPEN.md))
**Область:** не локализовано; проверяемый путь — `common/security-features/resources/common.sub.js::requestViaNavigable` (iframe с `name`, `<a target=name>.click()`, ожидание `message` с `event.source === iframe.contentWindow`) и `subresourceMap["a-tag"]` → `subresource/document.py`.

## Прямое измерение

`referrer-policy/4K` (2026-11-15, dev-release, Windows, `run_report.py --all --root referrer-policy/4K --recursive --processes 7`): **все 12** `a-tag.http.html` этого корня — harness TIMEOUT, ни одного `test_status`:

```
4K/gen/top.http-rp/{no-referrer-when-downgrade,strict-origin-when-cross-origin,unsafe-url,
                    origin-when-cross-origin,same-origin,unset}/a-tag.http.html
4K/gen/top.meta/{same-origin,no-referrer-when-downgrade,unsafe-url,unset,
                 origin-when-cross-origin,strict-origin-when-cross-origin}/a-tag.http.html
```

TIMEOUT виден и до правок среза 1 BUG-1235 (первый прогон той же директории дал `8/9 harness OK`,
a-tag — единственный не-OK файл). В старой записи [BUG-1097](BUG-1097-FIXED.md) те же файлы
проходили как `a-tag` 3 сабтеста с `[object Object]` — то есть с тех пор форма отказа изменилась
(проверить, когда и чем, в этой сессии не удалось: сравнительного прогона на старом бинаре не было).

## Что известно и чего не хватает

Не установлено, на каком шаге встаёт: навигация iframe по `.click()` не начинается, `document.py`
не постит `message`, или `event.source !== iframe.contentWindow` (тогда должен быть FAIL, а не
TIMEOUT). Для разбора — один файл через прогон с `--log-raw` и вычитыванием `process_output`
запрошенных URL (`document.py` должен появиться в сети), затем проба формы
`requestViaNavigable` без `testharness.js` по [`docs/probe-method.md`](../../docs/probe-method.md).
