# BUG-1710 — `IndexedDB/structured-clone.any.html` (и `parallel-cursors-upgrade.any.html`) не возвращают управление при загрузке: `navigate … DocumentReady` > 30 с

**Статус:** OPEN
**Заведён:** 2026-10-26 (P2, WPT-IDB-SCOUT)
**Область:** js/shell (причина не локализована; подозрение — участок `structured-clone.any.js` строки ~301–335: `new File([...])`, `cloneObjectTest(File…)`, `cloneFailureTest`)

## Симптом

Страница не завершает загрузку: BiDi `browsingContext.navigate` с `wait: complete` истекает через 30 с, статус ERROR без сабтестов. Касается шести вариантов `structured-clone.any*` и `parallel-cursors-upgrade.any.html` (7 id).

## Как найдено

WPT-IDB-SCOUT, кластер P. Bisect-проба: зависание наступает после строки ~301 (`new File([...])`); сами `File`, `input.files`, `Event`, `MessageChannel` по отдельности в порядке. Участок 301–335 не добит — следующий шаг: добавлять тесты из файла по одному в страницу-пробу (в `tests/wpt/` пробы не держать — попадут в корпус).

## Что делать

Локализовать вызов, который не возвращает управление (скорее всего синхронный цикл в клонировании `File`/`Blob` или в `cloneFailureTest`), починить. Потолок +0.9 п.п. категории.

## Как проверить

`run_smoke.py` на `IndexedDB/structured-clone.any.html` завершается без ERROR.
