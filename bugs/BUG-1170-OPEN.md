# BUG-1170 — baseline `tests/wpt/metadata/pointerevents/` устарел: `--check` красный по 540 «регрессиям»

**Статус:** OPEN
**Компонент:** WPT-baseline (`tests/wpt/metadata/pointerevents/*.ini`, снят 2026-09-21 коммитом `2c1e7560b`,
WPT-RUN-7 срез 44)
**Найден:** 2026-09-25, P6, BUG-1073 срез 6

## Симптом

`run_report.py --all --root pointerevents --recursive --processes 4 --check` на тихой системе, бинарь
`dev-release` от `origin/main` `c041bbd66`, три прогона подряд с одинаковым итогом (258/258 `TEST_END`,
ни одного обрыва):

```
tests: 96/258 harness OK; subtests: 424/1057 passed        (baseline 2026-09-21: 23/258, 301/489)
check: 540 regression(s), 73 unexpected pass(es) (narrow expectations), 0 other deviation(s)
```

Раскладка 540 «регрессий»: 282 `expected PASS, got FAIL`, 113 `expected PASS, got NOTRUN`,
95 `expected ERROR, got TIMEOUT`, 50 `expected PASS, got TIMEOUT` — по 161 файлу.

## Причина

Все 161 файл в baseline записаны как `expected: ERROR` без единой секции подтеста. 2026-09-21 эти тесты
падали `ERROR` ещё до первого подтеста на селекторе `test_driver` (BUG-1065, 152 id, и BUG-1063, 12 id).
BUG-1065 исправлен 2026-09-23, тесты теперь доходят до своих подтестов, а подтест без записи в `.ini`
ожидается `PASS` — отсюда «регрессии». Это не ухудшение движка: harness OK вырос с 23 до 96, прошедших
подтестов — с 301 до 424.

Самый крупный файл — `idlharness.https.window.html` (142 «регрессии»: форма интерфейсов `MouseEvent`,
`PointerEvent`, `WheelEvent`, `Window`, `Document`, `HTMLElement` — `assert_true: The prototype object must
have a property …`, `should not be enumerable`, `property has wrong .name`). Это известный класс WebIDL-формы
(BUG-912 и соседи), а не новый дефект.

## Ожидание

`--check` по категории зелёный на текущем `main`, пока ничего не ухудшилось.

## Что сделать

Перегенерировать baseline (`--update-expected`, затем `--check` с exit 0 — порядок WPT-RUN-7), разобрать
новые `FAIL`-подтесты по причинам.
