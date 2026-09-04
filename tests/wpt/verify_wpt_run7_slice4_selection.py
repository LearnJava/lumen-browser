"""WPT-RUN-7 срез 4: пины на выборку `run_report.all_vendored_test_ids`.

Три дефекта генератора, найденные при расширении expectations-baseline на
пакет из 18 категорий (журнал — `docs/tasks/p2-test-track.md`). Все три
проверяются без запуска браузера и без wptserve: выборка считается из
вендоренного `tests/wpt/metadata/MANIFEST.json`, поэтому скрипт стоит
секунды.

1. **Страница, которой манифест не знает, роняла весь прогон категории.**
   `-ref.html`-эталон не является item'ом манифеста, а выборка оставляла
   такой id «fail open». Для `avif`/`gif` он оказывался ЕДИНСТВЕННЫМ
   выбранным id, и wptrunner умирал до первого теста
   (`CRITICAL Unable to find any tests at the path(s)`, exit 64) — категория
   не получала baseline вовсе, а не просто фантомный MISSING.
2. **`.extension.js` не разворачивался.** Список шаблонных суффиксов был
   захардкожен как `.any.js`/`.window.js`, поэтому шесть настоящих
   testharness-тестов `web-extensions` не выбирались никогда, и категория
   рапортовала «no tests selected».
3. **Тест, существующий только как query-вариант, не выбирался.** Все
   `websockets/**/*.html` отдаются как `?default`/`?wss`/`?wpt_flags=h2` и
   не существуют по голому пути; выборка строила голый id по имени файла —
   123 нерабочих id вместо 290 настоящих.

Запуск:
    tests/wpt/.venv/Scripts/python.exe tests/wpt/verify_wpt_run7_slice4_selection.py
"""

import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import run_report  # noqa: E402

# Категории, у которых после фикса не должно остаться ни одного runnable id:
# в манифесте они целиком reftest/manual/crashtest/support, а раньше выборка
# подбирала в них эталонные и вспомогательные страницы.
EMPTY_CATEGORIES = ("avif", "gif", "html-longdesc", "annotation-protocol")

# `web-extensions/*.extension.js` -> `*.extension.html` (дефект 2).
WEB_EXTENSIONS_EXPECTED = 6

# Голый путь, которого у wptserve не существует (дефект 3).
WEBSOCKETS_BARE_ID = "/websockets/binary/001.html"


def main() -> int:
    failures = []

    for category in EMPTY_CATEGORIES:
        ids = run_report.all_vendored_test_ids(category, recursive=True)
        if ids:
            failures.append(f"{category}: ожидалось 0 runnable id, получено {len(ids)}: {ids[:3]}")

    web_extensions = run_report.all_vendored_test_ids("web-extensions", recursive=True)
    if len(web_extensions) != WEB_EXTENSIONS_EXPECTED:
        failures.append(
            f"web-extensions: ожидалось {WEB_EXTENSIONS_EXPECTED} id "
            f"(*.extension.js), получено {len(web_extensions)}"
        )
    if not all(i.endswith(".extension.html") for i in web_extensions):
        failures.append(f"web-extensions: неожиданные id в выборке: {web_extensions}")

    websockets = run_report.all_vendored_test_ids("websockets", recursive=True)
    if WEBSOCKETS_BARE_ID in websockets:
        failures.append(f"websockets: голый (неисполнимый) id всё ещё в выборке: {WEBSOCKETS_BARE_ID}")
    variants = [i for i in websockets if i.startswith(WEBSOCKETS_BARE_ID + "?")]
    if not variants:
        failures.append(f"websockets: ни одного query-варианта для {WEBSOCKETS_BARE_ID}")

    for line in failures:
        print("FAIL:", line)
    if failures:
        return 1
    print(
        f"OK: {len(EMPTY_CATEGORIES)} категории без runnable-тестов пусты, "
        f"web-extensions = {len(web_extensions)} id, "
        f"websockets = {len(websockets)} id ({len(variants)} варианта у {WEBSOCKETS_BARE_ID})"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
