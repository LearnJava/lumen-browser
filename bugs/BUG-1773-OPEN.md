# BUG-1773 — print-reftest: `--print-to-pdf` страницы с wptserve занимает ~20 с вместо секунд — 59 тестов PASS→TIMEOUT между сборками 2026-10-23 и 2026-10-26

**Статус:** OPEN
**Заведён:** 2026-10-28 (P1, WPT-RUN-19 — разбор падений категорий против `2026-10-22-popups.json`)
**Область:** shell (`crates/shell/src/dump_mode.rs::do_print_to_pdf` — загрузка страницы и ожидание перед печатью) или js/net (что-то в окне 2026-10-23…26 держит одноразовый процесс); `tools/wptrunner/wptrunner/executors/executorlumen.py::LumenPrintRefTestExecutor.screenshot` ни при чём — не менялся с 2026-10-22

## Симптом

`print-reftest`-тесты (`*-print.html`, исполнитель — один `lumen --print-to-pdf <out.pdf> <url>` на страницу) массово получают `TIMEOUT` («TestRunner hit external timeout»). Прогон WPT-RUN-19 (`572a8f960`) против WPT-RUN-16 (`d4114145a`):

| | 2026-10-22 | WPT-RUN-19 |
|---|---:|---:|
| `print-reftest`, балл (из 416 id) | 187.0 | 136.0 |
| статус TIMEOUT | 38 | 188 |
| PASS → TIMEOUT | — | 59 |
| FAIL → TIMEOUT | — | 95 (балл не меняется) |

59 потерянных баллов по каталогам: `css/css-page` 19, `css/css-break` 13, `css/printing` 7, `html/rendering` 6, `css/CSS2/pagination` 4, `css/css-cascade` 4, остальные по одному. Вместе с ними это основная часть просадок `css` в подкаталогах `css-page` −18, `printing` −7, `css-cascade` −3.5.

## Проба (воспроизводится на тихой машине, не шум нагрузки)

Три теста через `run_corpus.py --prefixes css/css-page/fixedpos-001-print.html,css/css-page/fixedpos-002-print.html,css/css-cascade/scope-implicit-001-print.html --skip-manifest-update --processes 1`, разные бинари `dev-release`, один и тот же раннер:

| бинарь (дата сборки) | результат |
|---|---|
| 2026-10-20 23:00 | PASS ×3 |
| 2026-10-23 18:05 | PASS ×3 |
| 2026-10-26 22:22 | **TIMEOUT ×3** |
| `572a8f960` (2026-10-27) | **TIMEOUT ×3** |

В логе раннера: `TEST_START` → через ~19.7 с `Unexpected viewport size ... (794, 1123), expected (480, 288)` (это штатное предупреждение — страница A4, см. комментарий в `executorlumen.py`) → через 5 с «Got timeout in harness». То есть сам `--print-to-pdf` отработал, но за ~20 с при бюджете `timeout + extra` ≈ 15 с.

Тот же тест вне раннера: `file://` — 0.7 с; обычный `python -m http.server` — 3.3 с (2026-10-23) и 3.9 с (2026-10-26); под wptserve — ~20 с. Значит замедление зависит от окружения wptserve (подресурсы `testharness*.js`, `common`-хелперы, `www1.localhost`-хосты), а не от самой печати.

## Окно подозрения (не бисектилось)

В диапазоне 2026-10-23 18:05 … 2026-10-26 22:22 — 89 коммитов. Подозрительные по теме:

- `5a07f0860` / `81ea056d3` BUG-1714: fetch link-подсказок через рабочий поток — одноразовый процесс может ждать завершения подсказок.
- `adaf303ee` VT-API-2: `pagereveal` и `finished` по анимациям псевдодерева — ожидание события перед снимком.

Начать с проверки, чем занят `--print-to-pdf` эти 15 с (`LUMEN_JS_STALL_SAMPLE_MS`, лог сети) на странице, поднятой `tests/wpt/serve_wpt_like.py`, затем бисект по этим коммитам.

## Done when

`css/css-page/fixedpos-001-print.html` и два теста выше проходят через `run_corpus.py` свежим бинарём; `print-reftest` в следующем полном прогоне не ниже 187 баллов (WPT-RUN-16).

## Бисект (WPT-DROP-1027-S3, 2026-10-28)

Проверено на 3 print-reftest из `html/` (`table-row-pagination-002-print`, `details-page-break-after-1-print`,
`object-image-display-none-loading-for-print`) по сетке `--first-parent`, бинари `dev-release`, `lumen.py` из
`a76e4c7b9`: `59ac93a0b` PASS, `ce13bf8d4` PASS, `6d22b7ada` TIMEOUT, **`a73fc1c15` TIMEOUT**.
Первый плохой — `a73fc1c15` «Влить ветку p1-textq-2: TEXTQ-2, гамма и контраст покрытия глифа»
(`ce13bf8d4` — его родитель по first-parent). Диф — растеризация глифов и снимки `graphic_tests/snapshots/cpu`;
как это замедляет `--print-to-pdf` на wptserve — неизвестно, подтвердить сборкой с откатом TEXTQ-2.
Это уточняет окно подозрения выше (BUG-1714 / VT-API-2 в нём ни при чём — `ce13bf8d4` уже после них).
