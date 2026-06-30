# Задача: SDC-1a — WinitSession-команды (driver, 8A.7 Ф4)

**Developer:** P1
**Ветка:** `p1-sdc1a-winit-commands`
**Размер:** M
**Крейты:** `lumen-driver`

## Контекст
Серия SDC — управление живым видимым окном Lumen напрямую командами (Selenium/WebDriver-подобно),
по ADR-006 (automation = родная поверхность движка) и плану `docs/plans/8A.7-shell-as-driver-client-plan.md`.
Фазы 1-3 готовы: `WinitSession` (`crates/driver/src/winit_session.rs`) умеет `navigate /
screenshot / layout_snapshot / computed_style / a11y_tree`. **Фаза 4 не сделана.**

Разрез по владению (CLAUDE.md: `lumen-shell` = территория P3): **SDC-1a (эта задача, P1)** —
driver-часть; **SDC-1b (P3)** — проводка в шелл. Interface-first: здесь публикуется командный
интерфейс, P3 подключает его в живое окно.

## Пред-запуск
- [ ] Прочитать: `docs/plans/8A.7-shell-as-driver-client-plan.md` (Фаза 4)
- [ ] Прочитать: `docs/decisions/ADR-006-automation-api.md` §4 (нативный ввод), §5 (auto-wait)
- [ ] Прочитать: `crates/driver/src/winit_session.rs:800-915` (готовый navigate + заглушки)
- [ ] Прочитать: `crates/driver/src/session.rs` (трейт `BrowserSession`, типы `Target`/`WaitCondition`)

## Что сделать
1. **Дореализовать заглушки в `WinitSession`** (winit_session.rs:838-915), сейчас `Err("8A.7"/"8A.8")`:
   - `click(target)` — hit-test по a11y/селектору → **нативный** ввод (ADR-006 §4, не синтетика);
   - `type_text(target,text)` — клавиатурные события + обновление формы;
   - `scroll(target,delta)` — scroll + relayout;
   - `eval(js)` — выполнение в живом QuickJS активной вкладки (под `--features quickjs`);
   - `wait(cond,timeout)` — **auto-wait в движке** (ADR-006 §5) по layout/network/JS-idle тику.
2. **Опубликовать командный интерфейс для шелла:** `enum AutomationCommand { Navigate(String),
   Click(Target), Type(Target,String), Scroll(Target,ScrollDelta), Eval(String), Screenshot, Wait(...) }`
   + результат `AutomationReply` (ack / PNG-байты / eval-строка / ошибка). Это контракт, который
   P3 (SDC-1b) скормит в живой event-loop. Документировать `///`-комментами (для параллельной сессии).
3. Без winit-окна методы остаются headless-исполнимыми (как Фазы 1-3) — шелл-интеграция отдельно.

## Приёмка
- [ ] `click/type_text/scroll/eval/wait` в `WinitSession` больше не возвращают `Err("8A.7")`
- [ ] Юнит-тесты на file:// (navigate→click→assert layout/eval) в `crates/driver/tests/`
- [ ] `AutomationCommand`/`AutomationReply` экспортированы из `lumen-driver` с doc-комментами
- [ ] `cargo clippy -p lumen-driver --all-targets -- -D warnings`
- [ ] `cargo test -p lumen-driver`

## Завершение
- Удалить строку `ROADMAP.md:184` из `STATUS-P1.md`
- `ROADMAP.md` SDC-1a status → done; SDC-1b/SDC-2/SDC-3 снять `wait` → `ready` (разблокированы цепочкой);
  `python scripts/gen_roadmap.py`
- `SYMBOLS.md` — регенерировать (новый pub enum); `python scripts/gen_symbols.py`
- Удалить этот файл
