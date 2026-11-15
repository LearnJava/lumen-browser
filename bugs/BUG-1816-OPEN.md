# BUG-1816 — reCAPTCHA v3: токен приходит быстро, но Google его отвергает (`score: null`, `browser-error`)

**Статус:** OPEN
**Заведён:** 2026-11-15 (P3, срез 6 [BUG-1647](BUG-1647-OPEN.md), после
[BUG-1815](BUG-1815-FIXED.md))
**Область:** js / сеть — ветка сигналов бандла reCAPTCHA (`api2/webworker.js`),
`api.js ↔ anchor` handshake, окружение `Worker`, видимое бандлу

## Симптом

JS-путь вендора теперь цел: `grecaptcha.ready()` срабатывает,
`grecaptcha.execute(site_key, {action:'examples/v3scores'})` резолвится
**быстро** (проба `.tmp/sc1815.py`: токен и ответ verify уложились в 9 с; срез 1
мерил 30,5 с по `execute-ms=30000`), ошибок в stderr нет. Но выданный токен
Google не принимает — `POST`/`GET /recaptcha-v3-verify.php?…&token=…` отвечает:

```json
{"success":false,"hostname":"","challenge_ts":"","apk_package_name":"",
 "score":null,"action":"","error-codes":["browser-error","hostname-mismatch",
 "action-mismatch","score-threshold-not-met", …]}
```

Оракул `score` стенда ищет `"score": <0..1>` — `null` не совпадает, вердикт
`не_отрисован`.

## Что измерено

- Токен по-прежнему **короткий**, с префиксом `HF…` (~630 символов), а не
  `0cAFcWeA…` (~1,5 КБ), как у Chrome (`docs/perf/runs/2026-10-14-captcha-chrome.json`).
- Lumen тянет `GET https://www.google.com/recaptcha/api2/webworker.js` (в логе
  сети `.tmp/sc1815.log`), Chrome на той же странице — **нет** (диф сети среза 1,
  `.tmp/cap1647_chrome.py`). Бандл уходит в ветку сигналов через `Worker`,
  когда не может собрать их в окне; короткий токен — продукт этой ветки.
- `api2/clr` и `api2/reload` Lumen запрашивает, у `reload` ответ `403`.

## Рабочая гипотеза

`browser-error` — следствие урезанной ветки сигналов: бандл выбирает
`webworker.js` (сбор сигналов в фоновом воркере вместо окна), потому что в окне
ему чего-то не хватает. Искать надо различие в том, что бандл пробует ДО выбора
ветки (наличие/поведение Web API в окне и в воркере), а не в самом токене.

## Что сделать

1. Взять деобфусцированный фрагмент `api.js`/`recaptcha__en.js`, который решает
   «запускать `api2/webworker.js` или собирать сигналы в окне» (в срезе 1 он
   локализован по сети, не по коду), и найти API, чей ответ у нас отличается от
   Chrome.
2. Починить найденный Web API по спеке и поставить тест в `crates/js`.
3. Проверка: `python scripts/captcha_stand.py --arm lumen --only
   recaptcha-v3-score` → «пройдено», балл v3 в JSON-записи (`v3_score`), токен
   длинный (`0cAFcWeA…`), `api2/webworker.js` в сети не запрашивается.
