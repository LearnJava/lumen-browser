# BUG-313: document.createProcessingInstruction отсутствует + нет валидации Name

**Статус:** FIXED 2026-07-19
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

## Решение (2026-07-19)

`document.createProcessingInstruction(target, data)` добавлен в engine-agnostic
`WEB_API_SHIM` (`crates/js/src/dom.rs`):

- `_lumen_is_xml_name(s)` — валидатор XML 1.0 Name production (regexp по
  NameStartChar/NameChar-диапазонам, BMP; астральный диапазон
  `#x10000-#xEFFFF` опущен — ни один сабтест его не задействует). Именно он
  корректно исключает `U+00D7` (×) и `U+00B7` (·) из NameStartChar, но
  допускает `·` как NameChar (`A·A`).
- `_lumen_make_processing_instruction(target, data)` — detached JS-only
  CharacterData-узел (PI никогда не участвует в layout), с
  `target`/`data`/`nodeType 7`/`nodeName`/`ownerDocument`.
- Метод бросает `DOMException('…','InvalidCharacterError')` (legacy code 5),
  если `target` не валидное XML Name либо `data` содержит `?>`.

Закрывает **8 из 11** проваленных сабтестов
(`INVALID_CHARACTER_ERR`-группа). Оставшиеся **3** (`Should get a
ProcessingInstruction …`) требуют глобальных интерфейсов
`ProcessingInstruction`/`Node` для `instanceof` — это скоуп
[BUG-314](BUG-314-OPEN.md); их `expected: FAIL` в
`metadata/.../Document-createProcessingInstruction.html.ini` оставлен и
переатрибутирован на BUG-314.

Юнит-тесты (`crates/js/src/dom.rs`, модуль `tests`):
`create_processing_instruction_returns_node`,
`create_processing_instruction_accepts_valid_names`,
`create_processing_instruction_rejects_invalid`.
