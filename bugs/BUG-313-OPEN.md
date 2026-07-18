# BUG-313: document.createProcessingInstruction отсутствует + нет валидации Name

**Статус:** OPEN
**Дата:** 2026-07-18
**Компонент:** js (WEB_API_SHIM, `crates/js/src/dom.rs`)
**Найден:** P2-wpt S5, курируемый синхронный DOM-сабсет через `wptrunner`

## Симптом

`Document.prototype.createProcessingInstruction(target, data)` (DOM Standard
§4.5) отсутствует — `grep createProcessingInstruction crates/js/src/dom.rs`
→ 0 совпадений.

Провалы сабтестов `Document-createProcessingInstruction.html`
(11 из 12 `expected: FAIL`, 1 PASS):

```
Should get a ProcessingInstruction for target "xml:fail" and data "x".
  -> document.createProcessingInstruction is not a function
Should throw an INVALID_CHARACTER_ERR for target "A" and data "?>".
  -> assert_throws_dom (метод отсутствует, исключение не то)
```

## Ожидание

`createProcessingInstruction(target, data)`:
- бросает `InvalidCharacterError`, если `target` не валидное XML Name или
  `data` содержит `"?>"`;
- иначе возвращает узел `ProcessingInstruction` с заданными `target`/`data`.
Требует также сам интерфейс `ProcessingInstruction` (см.
[BUG-314](BUG-314-OPEN.md), конструкторы DOM как глобали). Реализовать в
engine-agnostic `WEB_API_SHIM`.

## Воспроизведение

```bash
LUMEN_PROFILE=dev-release tests/wpt/.venv/Scripts/python.exe \
  tests/wpt/run_smoke.py /dom/nodes/Document-createProcessingInstruction.html
```
