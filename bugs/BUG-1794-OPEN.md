# BUG-1794 — github.com: сталл движка на загрузке — eval >5 с, страница blank

**Статус:** OPEN
**Заведён:** 2026-11-06 (P3, находка compat-crawl 10 сайтов)
**Область:** js/shell — производительность V8 на тяжёлом JS (Primer/React), `about_to_wait.rs`

## Симптом

Прогон `scripts/compat_crawl.py` (Lumen vs Chrome for Testing 153), отчёт `.tmp/compat-crawl/20261106-170917/`:

- `https://github.com` — HUNG в обоих прогонах: движок занят >5 с на одном eval во время загрузки (`about_to_wait.rs:295`), `ready_s` 47–63 с против ~4 с у Chrome.
- Итог: `blank:near-empty`, скриншот и a11y-дерево не сняты.
- Остальные 9 сайтов корпуса проходят — это не общий сталл, а специфика тяжёлого JS github (Primer CSS, React, большой DOM).

## Что сделать

1. **Сначала перепроверить на dev-release** (`target/dev-release/lumen.exe`, собрать `cargo build --profile dev-release -p lumen-shell` при необходимости) — debug-профиль может exaggerate CPU-стоимость; если на dev-release страница грузится за разумное время, severity ниже, но баг остаётся.
2. Если воспроизводится — снять CPU-профиль V8 на загрузке github.com (V8 `--prof`/`--trace` через существующие механизмы или sampling-профилировщик), найти горячий цикл.
3. Кандидатные направления: длинные синхронные layout/relayout при DOM-мутациях React, повторные `getComputedStyle`/selector-матчи, O(N²) в flat-tree/collect-обходах на большом DOM.

## Проверка

- compat_crawl на github.com: status/blank/exceptions зелёные, `ready_s` в пределах разумного от Chrome.
- Регрессионный бенчмарк или WPT-стиль тест на паттерн, который вешал eval.

## Связи

- Compat-crawl отчёт: `.tmp/compat-crawl/20261106-170917/summary.md` (не коммитится, `.tmp/`).
- Близкие места: `crates/shell/src/app/about_to_wait.rs:295` (eval-сталл watchdog), engine-thread pump (`relayout.rs::pump_raf_engine_thread`).
