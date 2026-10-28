# BUG-1776 — unicode-bidi-ua-rules: −1 подтест после merge `89500a780` (`<br>` без текста открывает пустую строку)

**Статус:** OPEN
**Заведён:** 2026-10-28 (P2, WPT-DROP-1027-S3)
**Область:** layout (`crates/engine/layout/src/box_tree/inline_build.rs`)

## Симптом

`/html/rendering/bidi-rendering/unicode-bidi-ua-rules.html`: 49/130 (старый бинарь, WPT-RUN-16) → 48/130
(текущий `main`, оба прогона). Потеря одного подтеста; сам тест и так проходит меньше половины.

## Бисект (WPT-DROP-1027-S3)

Окно `e3536bfb3`(49) … `89500a780`(48); сборки `5aae09dce` 49, `a06d378ef` 49, `d17e471ac` 49 →
первый плохой — `89500a780` «Merge branch 'main'». Из него в `main` попадает
`5fc170885 WIP BUG-1411: <br> без текста перед ним открывает пустую строку`
(`crates/engine/layout/src/box_tree/inline_build.rs` +54, `tests/br_lines.rs`). Подозреваемый
подтест — со строкой `<br>` внутри bidi-элемента: лишняя пустая строка сдвигает
порядок/результат сравнения.

## Воспроизведение

`tests/wpt/run_smoke.py --binary <lumen.exe> /html/rendering/bidi-rendering/unicode-bidi-ua-rules.html`;
бинари слота p2-work: `.tmp/bisect-bin/{d17e471ac,89500a780}/lumen.exe` (`lumen.py` из
`a76e4c7b9`). Ожидание 49/130, факт 48/130. Приоритет низкий (−1 подтест).
