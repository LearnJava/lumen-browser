# BUG-800 — встроенный adblock (EasyList) блокирует служебные запросы WPT-инфраструктуры под автоматизацией

**Статус:** FIXED 2026-08-21
**Заведён:** 2026-08-21 (WPT-RUN-6, срез 8 — расследование крупнейшего необъяснённого TIMEOUT-кластера)
**Область:** `crates/shell/src/main.rs` (сидирование `ShieldsPanel::default_enabled` при старте), `tools/wptrunner/wptrunner/browsers/lumen.py`
**Владелец:** P1/P3 (движок), но исправлено P2 с явного разрешения пользователя — по прецеденту [BUG-785](BUG-785-FIXED.md) (та же форма фикса: точечный env-флаг + правка продуктового плагина wptrunner, не требует знания остального движка).

## Симптом

После довендоривания `common/security-features/` (закрывшего TIMEOUT от
отсутствующего `common.sub.js`, см. журнал `WPT-RUN-6` в `ROADMAP.md`, срез 8)
`referrer-policy`-тесты продолжали давать 0/9 harness OK — но уже не TIMEOUT,
а `ERROR`:

```
AssertionError: Got results from /, expected /referrer-policy/4K/gen/top.http-rp/no-referrer-when-downgrade/a-tag.http.html
```

## Механизм

Лог перед ошибкой:

```
Reload: http://www1.127.0.0.1:18500/common/security-features/subresource/document.py?redirection=no-redirect&action=purge&key=...&path=%2Fmixed-content
ошибка загрузки ...: network error: blocked: easylist
```

Каждый `referrer-policy`/`mixed-content`/`content-security-policy`/
`upgrade-insecure-requests`-тест начинается с навигации на
`subresource/document.py?...action=purge...` (общий helper
`common/security-features/resources/common.sub.js`, очищает состояние между
подтестами). URL этого запроса по форме совпадает с паттерном из EasyList
(вендоренного списка на 107587 правил, включённого по умолчанию — тот же
`DEFAULT_SHIELDS_ENABLED = true`, что и в обычной пользовательской сессии,
`crates/storage/src/browser_settings.rs:34`), запрос молча блокируется, и эта
блокировка **не считается ошибкой навигации** — движок сообщает успех и
остаётся на предыдущем документе ([BUG-438](BUG-438-FIXED.md), уже открытый
общий механизм: «успешный `navigate` не гарантирует, что страница
загрузилась»). Раньше эта цепочка была не видна, потому что вся категория
падала TIMEOUT-ом ещё до первого `test()` — сам факт, что harness вообще
стартует, разблокирован только срезом 8 этой задачи.

Ни у автоматизации (`--bidi-port`), ни у `wptrunner` нет способа выключить
adblock — только внутриклиентская настройка «Блокировать рекламу»
(`ShieldsPanel`), не читаемая ни CLI-флагом, ни env-переменной.

## Масштаб

По `grep -rl "common/security-features"` на вендоренном корпусе:

```
referrer-policy:            1376 файлов, из них 1350 TIMEOUT в снимке WPT-RUN-5 (97.1%)
mixed-content:                380 файлов,        269 TIMEOUT (69.3%)
content-security-policy:      261 файл,           107 TIMEOUT (59.1%)
upgrade-insecure-requests:    196 файлов (в снимке WPT-RUN-5 категория не запускалась вовсе — без данных)
fetch / service-workers/html:   5 файлов суммарно
```

Крупнейший найденный TIMEOUT-кластер во всём разборе `WPT-RUN-6` — больше
суммы всех трёх механизмов среза 1 (1210 id).

**Важно: этот фикс не переводит категорию в PASS сам по себе.** После снятия
adblock-блокировки та же цепочка упирается в два независимых, уже
известных/новых пробела (не в скоупе этого фикса):

1. Алиас `www1.<host>` (стандартный WPT-паттерн для второго origin) не
   резолвится — `network error: resolve www1.127.0.0.1: не найдено (os error
   11001)`, отдельная задача (не заведена этим срезом, см. журнал
   `WPT-RUN-6` срез 8 в `ROADMAP.md`).
2. Часть `.https.`-запросов внутри той же цепочки — уже открытый
   [BUG-792](BUG-792-OPEN.md) (потеря тела https-ответа без `close_notify`).

Проверка фикса поэтому — по **исчезновению** `blocked: easylist` из лога, не
по появлению harness OK.

## Починка

`LUMEN_NO_ADBLOCK` — новая env-переменная, читаемая один раз при старте шелла
(`crates/shell/src/main.rs`, там же, где сидируется `ShieldsPanel` из
персистентной настройки): если задана, `default_enabled` форсируется в
`false` независимо от персистентного значения, до вызова
`sync_adblock_filter()`. Персистентная настройка и обычный пользовательский
путь (UI-тумблер, per-site исключения) не затронуты — флаг читается один раз
и не имеет собственного сеттера.

`tools/wptrunner/wptrunner/browsers/lumen.py` (`LumenBrowser.__init__`)
безусловно кладёт `env["LUMEN_NO_ADBLOCK"] = "1"` в окружение каждого
запускаемого под автоматизацией процесса — рядом с уже существующим
`LUMEN_EXTRA_CA_CERT` (BUG-785), тем же путём (`env=` в `WebDriverBrowser`).

**Проверка (живой прогон, `run_report.py --root
referrer-policy/4K/gen/top.http-rp/no-referrer-when-downgrade --recursive`):**
до фикса — `blocked: easylist` на каждом из 9 файлов; после — блокировка
исчезла из лога полностью (сравнение `grep -c "blocked: easylist"` двух
логов: 1+ → 0), сообщения о неудаче сместились на два уже
задокументированных выше независимых пробела (`www1.` DNS, BUG-792).

## Связи

- [BUG-438](BUG-438-FIXED.md) — общий механизм («успешная навигация не значит
  загрузку»), которым здесь маскировалась причина отказа.
- [BUG-785](BUG-785-FIXED.md) — прямой прецедент формы фикса (env-флаг +
  правка `browsers/lumen.py`) и разрешения на выход P2 за рамки роли.
- Найден и исправлен P2, `WPT-RUN-6` срез 8, 2026-08-21.
