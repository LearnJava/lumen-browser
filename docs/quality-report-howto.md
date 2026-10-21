# Сводный отчёт качества — как запускать и читать

`python scripts/quality_report.py` сводит свежий и предыдущий полный прогон WPT (`docs/wpt/runs/*.json`),
два свежих полных прогона `graphic_tests/results/*.json` и открытые баги `BUGS.md` в
[`quality-report.md`](quality-report.md) и пишет срез истории `docs/quality/runs/<stamp>.json`
(только если данные изменились — повторный запуск ничего не меняет). `--check` — только итоговая строка
в stdout, без записи.

- **Когда:** после мержа нового `--run-json` прогона WPT или полного `graphic_tests/run.py`; коммитить
  отчёт и новый срез вместе с прогоном.
- **Что читать первым:** строку «Итог» (тренд и число просадок), затем «Просадки с прошлого отчёта» —
  регрессии WPT по категориям, рост CRASH/TIMEOUT/ERROR/FAIL, тесты graphic_tests, ушедшие из PASS.
- **Счётчики:** PASS/FAIL дают только reftest и crashtest, testharness — OK + доля сабтестов;
  UNSUPPORTED (тип без исполнителя), SKIPPED и NOT-RUN в числитель не входят. Дельта багов — к прошлому
  срезу; нет второго прогона — `n/a`. Свод «тест → баг» для топ-5: `tests/wpt/metadata/**/*.ini`,
  WPT-пути в незакрытых строках `ROADMAP.md` с колонкой `bugs`, таблицы [`wpt-status.md`](wpt-status.md).
- Тесты скрипта: `python scripts/test_quality_report.py`.
