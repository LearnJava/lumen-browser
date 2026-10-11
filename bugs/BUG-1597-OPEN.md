# BUG-1597 — UX-LOADING: `--bidi-port` перестаёт отвечать на `script.evaluate`/`browsingContext.navigate` — каждый тест WPT на `main` кончается TIMEOUT

**Статус:** OPEN
**Заведён:** 2026-10-11 (P2, WPT-RUN-14 срез 28, 30 малых модулей `css`: `css-logical`, `compositing`, `css-scroll-anchoring`, `css-content`, `css-scrollbars`, `css-will-change`, `css-syntax`, `css-color-adjust`, `css-rhythm`, `geometry` и др.)
**Область:** shell (`crates/shell/src/chrome_ui.rs:365` — `loading: Some(t.id) == active_id && self.nav_start.is_some()`; `crates/chrome/src/model.rs` — `fav_class`/`.spinner`; `crates/shell/src/app/about_to_wait.rs` — блок «UX-LOADING»)
**Владелец:** P3 (регресс задачи P6 UX-LOADING; чинить вместе с P6)

## Симптом

На `main` после слияния UX-LOADING (`6d4ae2719`, 2026-10-09) `lumen --bidi-port N --no-paint` принимает сессию и `browsingContext.getTree`, но **ни один** следующий вызов не получает ответа: `script.evaluate("1+1")` и `browsingContext.navigate(...)` ждут вечно (замер: 15 и 20 с — таймаут клиента). Прогон WPT через `run_corpus.py` кончается `TIMEOUT` на каждом тесте после перезапуска браузера: `browser did not answer a BiDi call within 20s ... (LUMEN_WPT_HARD_CAP)`.

## Проба (измерено 2026-10-11)

`.tmp/s28/bidiprobe.py`: `session.new` → `getTree` → `script.evaluate("1+1")` → `navigate(http://127.0.0.1:18555/…)` → `evaluate("1+1")`, `--no-paint`:

| сборка | `evaluate` на `about:blank` | `navigate` | `evaluate` после |
|---|---|---|---|
| `6ccb986ba` (до UX-LOADING) | `JS context not available: page is still loading` (штатно, мгновенно) | ok, 0,27 с | ok, 0,00 с |
| `6d4ae2719` (слияние UX-LOADING) | TimeoutError 15 с | TimeoutError 20 с | TimeoutError 15 с |
| `38fe000f4` (`main`, 2026-10-11) | TimeoutError | TimeoutError | TimeoutError |
| `38fe000f4` + `loading: false &&` в `chrome_ui.rs:365` | как до UX-LOADING | ok, 0,27 с | ok, 0,03 с |
| `38fe000f4` + `if false &&` у блока «UX-LOADING» в `about_to_wait.rs` | TimeoutError (причина не здесь) | TimeoutError | TimeoutError |

Бисекция `git bisect --first-parent 7feb5e4ec..HEAD` — первый плохой коммит `6d4ae2719`. Отключение одного поля `loading` в модели вкладки возвращает ответы, поэтому дело в перерисовке хрома с `.spinner` у вкладки с живой навигацией, а не в `stop_loading`. Механизм (бесконечная анимация, не отдающая цикл автоматизации) — гипотеза, не измерен.

## Последствия

- Любой прогон `tests/wpt/run_corpus.py`/`run_report.py`/`run_suite.py` на сегодняшнем `main` непригоден: `css-logical` — 17 TIMEOUT из 92 при `--processes 7` и все testharness-тесты TIMEOUT при `--processes 1` (замер на 27 первых id, прогон прерван); весь срез S28 на этом бинаре давал 34,97 % против 41,46 % на бинаре 2026-10-08, а набор TIMEOUT не воспроизводился между двумя прогонами (46 + 54 id только в одном из них).
- `run_smoke.py` с дефолтным `/dom/nodes/Element-hasAttribute.html` — TIMEOUT (контроль: тот же результат на пустом тесте).
- Срез S28 WPT-RUN-14 снят на бинаре от 2026-10-08 (движок до UX-LOADING) — по той же причине, по какой так же поступил S27.

## Что делать

Не обновлять спиннер вкладки, пока цикл автоматизации не отдал ответ (или не рисовать `.spinner` при `--bidi-port`/`--no-paint`); добавить в `verify_s3_bidi_session.py` шаг `script.evaluate` + `navigate`, чтобы такой регресс ловился гейтом (сейчас S3 проверяет только `session.new`).

## Как проверить

`tests/wpt/.venv/Scripts/python.exe .tmp/s28/bidiprobe.py <lumen.exe> --no-paint` (скрипт лежит в рабочем каталоге среза; суть — четыре вызова выше) либо `run_smoke.py --binary <lumen.exe>` — ожидается `OK`, а не `TIMEOUT`.
