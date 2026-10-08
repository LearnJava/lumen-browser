# BUG-1038 — `connection-allowlist`: минимум два независимых нестабильных сигнала на трёх подряд `--check`

**Статус:** FIXED 2026-09-30 (P6)
**Заведён:** 2026-09-08 (P2, WPT-RUN-7 срез 30 — попытка перегенерации `connection-allowlist`)
**Область:** не локализовано. Кандидаты — гонка вокруг `<link rel="prefetch">` в
`about:`-контенте iframe (`iframe-contentwindow-injection.sub.window.html`) и что-то в
цепочке кросс-origin редиректов (`navigation-redirect-default.sub.window.html`); обе
страницы активно используют `www*.localhost`-поддомены, которые на этой машине не
резолвятся (`os error 11001`) и генерируют десятки неудачных `fetch`/DNS-запросов на
фоне теста — не исключено, что именно этот побочный шум и есть источник таймингового
дрожания
**Владелец:** P1/P3 (после локализации)

## Симптом

`--update-expected --all --root connection-allowlist --recursive` прошёл штатно один раз
(24/73 harness OK, 85/164 подтестов, 59 `.ini`-записей). Три немедленных последовательных
`--check` на том же бинаре и baseline дали три РАЗНЫХ результата вместо ожидаемых 0
регрессий:

1. **check 1**: `REGRESSION` на подтесте `iframe-contentwindow-injection.sub.window.html`
   [`Injecting <link rel="prefetch"> into " about:" (space-prefixed) iframe contentWindow
   must be blocked by inherited Connection-Allowlist.`] — `expected PASS, got FAIL`.
2. **check 2**: `unexpected PASS` (сузить ожидания) на СОСЕДНЕМ подтесте того же файла
   [`Injecting <link rel="prefetch"> into about: iframe contentWindow must be blocked by
   inherited Connection-Allowlist.`] — тот, что как раз был записан в `.ini` при генерации
   как `expected: FAIL`.
3. Промежуточная проверка: правка `.ini` на `expected: [PASS, FAIL]` для обоих подтестов
   этого файла убрала расхождение из check 3, но **тот же check 3** тут же дал новую пару
   `REGRESSION` на СОВЕРШЕННО ДРУГОМ файле — `navigation-redirect-default.sub.window.html`
   целиком (harness `TEST_END`) и его единственном подтесте (`Redirect from
   http://not-web-platform.test:18300 to http://localhost:18300 should fail.`) —
   `expected OK/PASS, got TIMEOUT`. `TIMEOUT` — по конвенции этого тулинга
   (`docs/tasks/p2-test-track.md` TEST-3, срез 4/`BUG-1006`) всегда всплывает как
   регрессия, сузить его нарочно нельзя, в отличие от PASS/FAIL.

Правка `.ini` для первого файла (`expected: [PASS, FAIL]`) откачена вместе со всем
baseline'ом этой попытки — категория осталась без коммита, `git clean -fd
tests/wpt/metadata/connection-allowlist`.

## Почему это важно

Тот же класс дефекта, что [BUG-999](BUG-999-FIXED.md)/[BUG-1003](BUG-1003-FIXED.md)/
[BUG-1004](BUG-1004-CANNOT-REPRODUCE.md)/[BUG-1005](BUG-1005-FIXED.md): подтестовый baseline
невоспроизводим, гейт `--check` увидит регрессию на чистом `main` без единого движкового
изменения. Здесь хуже, чем в прецедентах, — плавают минимум ДВА независимых файла на трёх
прогонах подряд, то есть узкое сужение (`expected: [PASS, FAIL]`) одного подтеста не решает
задачу целиком, нужна локализация корневой причины, а не патч одного `.ini`.

## Воспроизведение

```
LUMEN_PROFILE=dev-release python tests/wpt/run_report.py \
  --binary target/dev-release/lumen --all --root connection-allowlist --recursive --check
```

Запустить 3+ раз подряд на одном бинаре без пересборки — расхождение плавает по файлу/
подтесту (не обязательно те же два, что в этой находке).

## Что не проверялось

Не изолировано до конкретного `<link rel=prefetch>`/redirect call site. Не проверено, есть
ли корреляция с фоновым DNS-шумом (`www.localhost`/`www1.localhost`/`www2.localhost` не
резолвятся на этой машине — `os error 11001`, десятки failed `fetch` в логе на каждый
прогон) или это структурная гонка в реализации connection-allowlist-инъекции/навигации,
независимая от окружения. Нужны изолированные прогоны одного файла (`run_smoke.py <id>`)
с диагностикой по каждому кадру, не прогон всей категории.

## Обновление 2026-09-20 (WPT-RUN-7 срез 39)

Категория взята заново после закрытия BUG-1006 и **получила baseline** (58 `.ini`). Плавали не те
подтесты, что в срезе 30: `www.localhost … should fail.` (PASS → TIMEOUT) и `www1.localhost …`
(TIMEOUT ↔ NOTRUN) в `navigation-wildcard`/`navigation-response-origin`, `(www)`/`(www1)` в `websocket`;
`iframe-contentwindow-injection` и `navigation-redirect-default` в трёх прогонах не плавали. Все шесть записей
сужены (`[PASS, TIMEOUT]`, `[TIMEOUT, NOTRUN]`, `[TIMEOUT, PASS]`, `[NOTRUN, TIMEOUT]`), три
`--check` подряд — 0 регрессий. Корневая причина плавания не локализована; поддомены не резолвятся на
Windows — [BUG-1070](BUG-1070-FIXED.md), это вероятный, но не доказанный источник. Статус остаётся OPEN:
блокер «baseline нельзя закоммитить» снят, но при перегенерации плавающий набор может оказаться другим.

## Обновление 2026-09-30 (P6): BUG-1070 закрыт, плавание осталось

[BUG-1070](BUG-1070-FIXED.md) починен (`*.localhost` → loopback в резолвере). Два подряд `--check` категории на одном бинаре
(без перегенерации baseline, который теперь устарел — сместились сотни статусов) дали расхождение только в
`iframe-contentwindow-injection.sub.window.html`: два подтеста `<link rel=prefetch>` (`about:` и `createElement`… `about:blank`)
в одном прогоне FAIL, в другом PASS. Плавание `www*.localhost` (TIMEOUT↔NOTRUN) в этих двух прогонах не наблюдалось.
Тот же файл изолированно (`run_smoke.py`, 4 прогона) стабилен: 4/5, FAIL на `createElement … about:blank`. Значит гонка
проявляется только под нагрузкой полной категории (соседние страницы, параллельные фоновые fetch), не сама по себе.
Дальше: перегенерировать baseline на новом резолвере и проверить, остаётся ли плавание; если да — искать нагрузочную
причину в подсистеме prefetch/about:-iframe. Статус остаётся OPEN.

## Закрытие 2026-09-30 (P6)

Корень плавания — не логика connection-allowlist, а два внешних фактора:
1. `www*.localhost` не резолвились ([BUG-1070](BUG-1070-FIXED.md), починен: `*.localhost` → loopback).
2. Тесты `navigation-*` решают «навигация заблокирована / прошла» по окну **50 мс** после `iframe.onload`
   (`resources/navigation_redirect_test.js`, `step_timeout(…, 50)`). Под нагрузкой машины (разбор страницы 0,47 с вместо
   0,2 с) postMessage приходит позже окна — все «should succeed» → FAIL, «should fail» → PASS, инверсия по всей
   семье `navigation-redirect-*`/`navigation-wildcard`/`navigation-response-origin` (+ `iframe-contentwindow-injection`).
   Отдельно каждый файл стабилен. Движковой гонки нет.

Baseline `connection-allowlist` перегенерирован на новом резолвере, плавающие подтесты записаны списками
(`[PASS, FAIL]`, `[TIMEOUT, NOTRUN]` и т. п.). Три `--check` подряд на первом baseline — 0 регрессий; затем один прогон дал 21 регрессию (семья navigation, механизм выше), ещё один — 1 (`websocket`, `PASS→TIMEOUT`), оба плавающих набора дописаны списками; два `--check` после этого — 0 регрессий.
