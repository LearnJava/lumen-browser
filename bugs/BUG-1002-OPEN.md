# BUG-1002 — `check_file_sizes.py` (CI job `file-size`) уже красный на `main`: 6 нарушений

**Статус:** OPEN
**Заведён:** 2026-09-05 (P1, побочная находка при подготовке гейта для FONTLOAD-6)
**Область:** `scripts/file-size-baseline.tsv` / `scripts/check_file_sizes.py` (`docs/lint-policy.md` §5.1)
**Владелец:** не назначен (CI-гейт `file-size`, не входит в 7-шаговый чек-лист `/lumen-task-finish` — там не всплывает)

## Симптом

`python scripts/check_file_sizes.py` (проверено на `cbb1a8876`, до и после FONTLOAD-6 — рост не мой, `git stash` подтверждает то же на HEAD без моих правок) выходит с кодом 1, 6 нарушений:

```
crates/engine/dom/src/lib.rs: вырос 5342 -> 5437 (+95)
crates/engine/layout/src/selector_query.rs: вырос 2758 -> 3123 (+365)
crates/js/src/dom/tests/v8_ws_sse.rs: 2011 строк > потолка 2000 (нет в baseline)
crates/js/src/worker.rs: вырос 3614 -> 3628 (+14)
crates/network/src/lib.rs: вырос 9602 -> 9887 (+285)
crates/shell/src/tests/bug341_census.rs: 2035 строк > потолка 2000 (нет в baseline)
```

Четыре первых — файлы уже в `scripts/file-size-baseline.tsv`, выросшие без `--update` того же коммита (нарушает храповик §5.1 — рост не запрещён, но должен быть назван). Два последних (`v8_ws_sse.rs`, `bug341_census.rs`) вообще не в baseline — либо новые файлы, перешагнувшие потолок 2000 без объявления, либо старые, доросшие до потолка после SPLIT-закрытия и не подхваченные `--update`.

## Почему не блокирует эту задачу

Гейт `file-size` — CI job (`ci.yml`), не входит в мандатный локальный чек-лист `.claude/skills/lumen-task-finish/SKILL.md` (шаги 1–2 — только `clippy --workspace` + `scoped-test.sh`). FONTLOAD-6 не трогает ни один из шести файлов. Не воспроизведено на моей ветке до правок (тот же результат на `cbb1a8876` через `git stash`).

## Первый шаг

Для каждого из 4 файлов в baseline — решить, оправдан ли рост (обычно да, движок растёт), и прогнать `python scripts/check_file_sizes.py --update` тем же коммитом с объяснением в теле. Для `v8_ws_sse.rs`/`bug341_census.rs` — либо разрезать (правило §5.1: «новый монолит заводить нельзя»), либо, если превышение не ново, добавить в baseline с объяснением, почему исключение оправдано.
