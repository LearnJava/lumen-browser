# BUG-783 — `global_intercept_pauses_real_fetch_until_resolved` флакует под нагрузкой гейта

**Статус:** OPEN
**Компонент:** network (`crates/network/src/lib.rs:8723-8731` — цикл опроса
`drain_new_intercept_announcements()`), тест `tests::global_intercept_pauses_real_fetch_until_resolved`
**Найден:** 2026-08-18 (P5), прогоном `scripts/scoped-test.sh` при внедрении
`clippy::unwrap_used` (правка коммита — только атрибуты `#[allow]`, поведения не меняет).

## Симптом

`scoped-test.sh` (все 24 крейта подряд, тест-бинари идут параллельно) уронил гейт:

```
test tests::global_intercept_pauses_real_fetch_until_resolved ... FAILED
panicked at crates/network/src/lib.rs:8731:37: fetch never registered as paused
test result: FAILED. 2153 passed; 1 failed
```

Повторный прогон того же теста в одиночку — PASS за 0.01 с; полный
`cargo test -p lumen-network --lib` — 2154/2154 PASS. То есть отказ
воспроизводится только при высокой параллельной нагрузке.

## Механизм

Тест ждёт регистрации перехвата опросом с фиксированным потолком:

```rust
for _ in 0..400 {
    if let Some((id, seen_url)) = drain_new_intercept_announcements().into_iter().next() { .. }
    thread::sleep(Duration::from_millis(5));
}
let request_id = request_id.expect("fetch never registered as paused");
```

400 × 5 мс = 2 с настенного времени. При параллельном прогоне десятков тест-бинарей
(и параллельной сборке) поток, выполняющий реальный `fetch`, может не получить
процессорное время за эти 2 с — потолок задан во времени, а не в событиях.

## Что дальше

Заменить ожидание по настенным часам на детерминированную синхронизацию
(`Condvar`/канал, которым перехватчик сигналит о регистрации), либо, как минимум,
поднять потолок и различать «не успели» и «не зарегистрировался». Пока этого нет,
единичный отказ этого теста в гейте — **не повод считать правку виновной**:
перепроверять одиночным прогоном.

Родственная запись: тот же тест упоминается в
[BUG-295](BUG-295-FIXED.md) как уже правившийся ранее.
