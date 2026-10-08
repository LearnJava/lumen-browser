# WPT-RUN-8-S1 — исполнитель crashtest для wptrunner

Срез 1 из WPT-RUN-8. Владелец — P2. Область — `tools/wptrunner/` и `tests/wpt/`, Python.

## Цель

Сейчас 1 892 crashtest-id никогда не исполняются. В `expectations.classify` они MISSING, то есть вечная «регрессия»; в счёт корпуса идут как 0.

Нужен исполнитель с критерием «не упал». По WPT: страница загрузилась, класс `test-wait` у `<html>` снят (или его не было), процесс жив.

## Точки входа

- Базовый `CrashtestExecutor` — [base.py:397](../../tools/wptrunner/wptrunner/executors/base.py#L397).
- Новый `LumenCrashtestExecutor` — в [executorlumen.py](../../tools/wptrunner/wptrunner/executors/executorlumen.py) рядом с `LumenRefTestExecutor` (`:718`).
  - Транспорт — BiDi (`LumenBidiProtocol`, `:157`), как у testharness. IPC-путь таймеры не прокачивает, и `test-wait` там не снимется.
  - Порядок: navigate → опрос `document.documentElement.classList.contains('test-wait')` до снятия или до таймаута теста → проверка, что процесс жив.
  - Результат: `PASS`, `CRASH` (процесс умер) или `TIMEOUT`.
- Карта исполнителей — [lumen.py:66-69](../../tools/wptrunner/wptrunner/browsers/lumen.py#L66): `"crashtest": "LumenCrashtestExecutor"`.
- [run_report.py:68](../../tests/wpt/run_report.py#L68): `RUNNABLE_ITEM_TYPES` += `"crashtest"`, поправить комментарий :63-67.
- Self-test [run_corpus.py:1016/1032](../../tests/wpt/run_corpus.py#L1016): сейчас ожидает `no_executor_by_type == {"crashtest": 1}`, привести к новой реальности.

## Не трогать

- print-reftest, aamtest, wdspec — отдельные решения.
- Существующие исполнители.

## Готово, когда

1. `tests/wpt/.venv/Scripts/python.exe tests/wpt/run_corpus.py --selftest` зелёный.
2. `run_report.py --all --root dom/crashtests` (4 id) даёт вердикты PASS/CRASH/TIMEOUT, без MISSING.
3. Ещё одна небольшая категория с crashtest'ами (например `css/css-text/crashtests`, 13) — то же.
4. `--update-expected` и два `--check` подряд по этим каталогам дают exit 0.
5. Код меньше 200 строк.

## Гейт

```
tests/wpt/.venv/Scripts/python.exe tests/wpt/run_corpus.py --selftest
tests/wpt/.venv/Scripts/python.exe tests/wpt/run_report.py --binary "$BIN" --all --root dom/crashtests --check
```

Сборка — `dev-release`, рецепт `$BIN` — `tests/wpt/README.md`.

## Зависимости

Нет (WPT-RUN-5 закрыт).
