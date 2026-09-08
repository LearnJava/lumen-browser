# BUG-1037 — `connection-allowlist`: минимум два независимых нестабильных сигнала на трёх подряд `--check`

**Статус:** OPEN
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

Тот же класс дефекта, что [BUG-999](BUG-999-OPEN.md)/[BUG-1003](BUG-1003-OPEN.md)/
[BUG-1004](BUG-1004-OPEN.md)/[BUG-1005](BUG-1005-OPEN.md): подтестовый baseline
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
