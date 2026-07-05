# Задача: CSS text-wrap: balance / pretty

**Developer:** P1
**Ветка:** `p1-text-wrap`
**Размер:** XS (алгоритмы реализованы; остаток — верификация + графтест)
**Крейты:** `lumen-layout`

## Goal
`text-wrap: balance` выравнивает ширину строк абзаца (равномерное распределение переносов), `text-wrap: pretty` предотвращает «вдов» (одинокое слово на последней строке). Плюс шортхенд `text-wrap` и лонгхенды `text-wrap-mode`/`text-wrap-style` (CSS Text L4 §6).

## Current state (сверено с кодом 2026-07-05)
Роадмап-семя (ROADMAP.md:133) помечает как PARTIAL «дошить алгоритм balance/pretty в line-break (сейчас поля не влияют на перенос)». **Пометка протухла** — алгоритмы реализованы и активны. Проверено:

- **Parsing.** `text-wrap-mode` (`wrap|nowrap`) — `crates/engine/layout/src/style.rs:11970-11977`; `text-wrap-style` (`auto|balance|stable|pretty`) — `style.rs:11979-11983`; шортхенд `text-wrap` — `style.rs:11985-11991`, разбор `apply_text_wrap_shorthand` — `style.rs:15181-15215`.
- **Enum'ы.** `TextWrapMode {Wrap,Nowrap}` — `style.rs:4446-4462`; `TextWrapStyle {Auto,Balance,Stable,Pretty}` — `style.rs:4473-4495`.
- **ComputedStyle.** Поля `text_wrap_mode` (`style.rs:3021`), `text_wrap_style` (`style.rs:3027`); наследуются (`style.rs:5973-5974`); initial `Wrap`/`Auto` (`style.rs:5623-5624`); CSS-wide keywords (`style.rs:16338-16367`).
- **Алгоритмы (НЕ заглушки).** `balance_wrap` — `crates/engine/layout/src/box_tree.rs:9439-9484` (бинпоиск минимальной ширины при том же числе строк, 20 итераций, реальный ре-wrap). `pretty_wrap` — `box_tree.rs:9493-9565` (детект вдовы, перенос слова на последнюю строку).
- **Точки вызова (активны).** `format_inline_box`: первая строка — `box_tree.rs:5654-5664`; обычный случай — `box_tree.rs:5678-5690`. `Balance`→`balance_wrap`, `Pretty`→`pretty_wrap`, `Auto|Stable`→greedy без изменений (подтверждено чтением 5650-5694).
- **`text-wrap-mode` → white-space.** `WhiteSpace::combine()` — `style.rs:349-366`; пересчёт при декларации — `style.rs:11976, 15214`.
- **CSS-SPECS.md:247** — `text-wrap-mode`/`text-wrap-style` = ✅; **CSS-SPECS.md:76** упоминает `text-wrap-style ⬜` в строке Text L3/L4 (протухший остаток — актуализировать).

Вывод: фича реализована end-to-end. Остаток — визуальная верификация качества переносов + графтест + чистка протухшего ⬜ в CSS-SPECS:76.

## Entry points
- `crates/engine/layout/src/style.rs:11970` — парсинг text-wrap-mode/style/шортхенд
- `crates/engine/layout/src/style.rs:4473` — `TextWrapStyle`
- `crates/engine/layout/src/box_tree.rs:9439` — `balance_wrap`
- `crates/engine/layout/src/box_tree.rs:9493` — `pretty_wrap`
- `crates/engine/layout/src/box_tree.rs:5654` — точки применения в `format_inline_box`

## Срезы (декомпозиция на мелкие задачи)
### Срез 1 — XS — Юнит-тесты алгоритмов
Проверить наличие юнит-тестов на `balance_wrap`/`pretty_wrap` в `box_tree.rs`; при отсутствии — добавить: (а) balance равномерно распределяет 2-3 строки; (б) pretty убирает одинокое последнее слово; (в) `Auto` не меняет greedy-результат. Тесты в `crates/engine/layout/src/`.

### Срез 2 — XS — Визуальная верификация
Собрать страницу с абзацем на 3-4 строки для `balance` и абзацем с вдовой для `pretty`. Сравнить с Edge (`--screenshot`/визуальный диф). Явные дефекты (например balance даёт неверное число строк) — BUG-NNN.

### Срез 3 — XS — Графический тест
Демо `text-wrap: balance` и `text-wrap: pretty` в текстовый графтест `graphic_tests/NN-*.html` + `1000000-final.html`, запись в `run.py` `TESTS` и `COVERAGE.md`. Учесть, что balance/pretty могут расходиться с Edge по тайминг-/шрифтовым причинам — при устойчивом расхождении оформить как KNOWN_DEBTOR (по паттерну TEST-71/77), а не менять порог.

### Срез 4 — XS — Синхронизация доков
`CSS-SPECS.md:76` — убрать `text-wrap-style ⬜` из строки Text L3/L4 (уже ✅ в 247). ROADMAP.md:133 — снять PARTIAL (владелец ROADMAP). `CAPABILITIES.md` — текстовая подсистема.

## Tests
- Юнит: `balance_wrap`/`pretty_wrap` (Срез 1), `cargo test -p lumen-layout`.
- Графический: демо balance+pretty (Срез 3), порог 0.5% или KNOWN_DEBTOR при обоснованном расхождении с Edge.

## Definition of done
- [ ] Юнит-тесты на balance/pretty присутствуют и зелёные
- [ ] Визуальная верификация balance (равные строки) и pretty (нет вдовы) без явных дефектов
- [ ] Графический тест добавлен (или обоснованный KNOWN_DEBTOR)
- [ ] `cargo test -p lumen-layout` зелёный
- [ ] CSS-SPECS:76 очищен от протухшего ⬜; ROADMAP/CAPABILITIES синхронизированы
