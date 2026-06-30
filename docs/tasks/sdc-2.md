# Задача: SDC-2 — BiDi/MCP на живое окно

**Developer:** P1
**Ветка:** `p1-sdc2-bidi-mcp-live`
**Размер:** M
**Крейты:** `lumen-bidi-server`, `lumen-mcp`
**ТРЕБУЕТ:** SDC-1b (смержен) — канал команд в живое окно (`AutomationCommand` поверх `lumen_ipc`)

## Контекст
SDC-1 даёт исполнение инструментов `BrowserSession` против живого окна (`WinitSession` +
командный канал). SDC-2 направляет на это **внешние протокол-фронты**, чтобы цеплялись готовые
экосистемные клиенты.

Сейчас фронты висят на in-memory/headless состоянии:
- `lumen-bidi-server` (WebDriver BiDi, PH3-2) — *«все команды работают только над in-memory
  `BidiState`; реальная навигация/скрипт/сеть требуют 8A.7»* (CAPABILITIES.md:200);
- `lumen-mcp` (PH1-9) — обёртка над `BrowserSession`, наследует лимиты headless `InProcessSession`
  (screenshot/eval → `Err`, CAPABILITIES.md:204).

По ADR-006 §8 нужны оба транспорта: **BiDi** (стандарт W3C, для Playwright/Selenium/Cypress) и
**MCP** (для AI-агентов). CDP — опционально и последним (ADR-006 §10).

## Пред-запуск
- [ ] Смержены SDC-1a (driver-команды) + SDC-1b (shell-канал в живое окно)
- [ ] Прочитать: `docs/decisions/ADR-006-automation-api.md` §8 (три транспорта), §9 (BiDi-gaps)
- [ ] Прочитать: `crates/bidi-server/src/` (где команды бьют в `BidiState`)
- [ ] Прочитать: `crates/mcp/src/server.rs` (tools/resources → `BrowserSession`)
- [ ] Прочитать: CAPABILITIES.md:198-204 (текущие лимиты BiDi/MCP)

## Что сделать
1. **BiDi → живое окно.** В `lumen-bidi-server` заменить операции над in-memory `BidiState` на
   вызовы `BrowserSession` живого `WinitSession` (через канал SDC-1): `browsingContext.navigate`,
   `script.evaluate`, `input.performActions`, `browsingContext.captureScreenshot` исполняются
   реально. Сохранить event-эмиссию (network/context/storage), где возможно.
2. **MCP → тот же путь.** `lumen-mcp` tools (`navigate/click/type/scroll/wait/eval/query`) +
   resources (`screenshot/a11y_tree/...`) направить на живой `WinitSession`, чтобы `screenshot()`/
   `eval()` больше не возвращали `Err`.
3. **Запуск:** оконный шелл поднимает фронт по флагу (`--bidi-port`/`--devtools-port` уже есть);
   фронт и окно — один процесс, фронт говорит с живым окном через канал SDC-1.
4. CDP (`lumen-devtools`) — НЕ в этой задаче (опц., отдельной задачей по ADR-006 §10).

## Приёмка
- [ ] BiDi: реальная навигация + `script.evaluate` + `captureScreenshot` против живого окна
      (ручной прогон WebSocket-клиентом или мини-Playwright-сценарий)
- [ ] MCP: `screenshot`/`eval` против живого окна возвращают результат, не `Err`
- [ ] CAPABILITIES.md:200,204 — снять отметки «in-memory only» / «inherits driver limits»
- [ ] `cargo clippy -p lumen-bidi-server -p lumen-mcp -p lumen-shell --all-targets -- -D warnings`
- [ ] `cargo test -p lumen-bidi-server -p lumen-mcp`

## Завершение
- Удалить строку `ROADMAP.md:186` из `STATUS-P1.md` (с переиндексацией)
- `ROADMAP.md` SDC-2 status → done; `python scripts/gen_roadmap.py`
- `CAPABILITIES.md` — обновить раздел Automation surfaces
- Удалить этот файл
