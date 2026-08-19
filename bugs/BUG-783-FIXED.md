# BUG-783 — `global_intercept_pauses_real_fetch_until_resolved` флакует под нагрузкой гейта

**Статус:** FIXED 2026-08-20
**Компонент:** network (`crates/network/src/intercept.rs`), тест
`tests::global_intercept_pauses_real_fetch_until_resolved`
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

Тест ждал регистрации перехвата опросом с фиксированным потолком:

```rust
for _ in 0..400 {
    if let Some((id, seen_url)) = drain_new_intercept_announcements().into_iter().next() { .. }
    thread::sleep(Duration::from_millis(5));
}
let request_id = request_id.expect("fetch never registered as paused");
```

400 × 5 мс = 2 с настенного времени. При параллельном прогоне десятков тест-бинарей
(и параллельной сборке) поток, выполняющий реальный `fetch`, может не получить
процессорное время за эти 2 с — потолок задан во времени, а не в событиях. Тот же
паттерн опроса дублировался в `intercept.rs`'s собственных блокирующих тестах
(`continue_decision_unblocks_with_ok`, `fail_decision_unblocks_with_err`, потолок
200 × 5 мс = 1 с) — тот же класс отказа, просто не пойманный живьём до этого бага.

## Фикс

`intercept.rs::registry()` теперь хранит два `Condvar` на одном `Mutex`, а не один:
`decision_condvar` (существовавший — им сигналит `resolve_intercept`, на нём блокируется
`pause_for_intercept`) и новый `registered_condvar`, которым `pause_for_intercept`
сигналит сразу после вставки записи в `pending`. Новая функция
`wait_for_new_intercept_announcement(timeout)` блокируется на `registered_condvar`
до появления неанонсированной записи (`wait_timeout_while`) вместо опроса по
расписанию — событийное ожидание вместо ожидания по настенным часам, тот же приём,
что уже применялся для `decision_condvar`.

`drain_new_intercept_announcements()` (неблокирующий, используется продакшн-кодом
BiDi-поллинга в `crates/shell/src/main.rs`) не тронут — только тесты, ждавшие
регистрацию опросом, переведены на `wait_for_new_intercept_announcement`:
- `crates/network/src/lib.rs::tests::global_intercept_pauses_real_fetch_until_resolved`
- `crates/network/src/intercept.rs::tests::continue_decision_unblocks_with_ok`
- `crates/network/src/intercept.rs::tests::fail_decision_unblocks_with_err`

Проверено: 5 повторных прогонов `intercept::tests` + `tests::global_intercept_
pauses_real_fetch_until_resolved` с `--test-threads=8` подряд — все зелёные, каждый
за 0.01 с (раньше — до 1–2 с настенного времени в успешном случае). Полный
`cargo test -p lumen-network --lib` — 2154/2154 PASS.

Родственная запись: тот же тест упоминается в
[BUG-295](BUG-295-FIXED.md) как уже правившийся ранее.

## Не входит в этот фикс

Второй флаки-случай того же гейта, `stale_pooled_connection_triggers_retry`
(macOS, обнаружен 2026-08-18 в этом же баге как «второй случай того же класса»),
устроен иначе — не опрос по расписанию, а гонка между потоком тестового сервера
и клиентским пулом соединений внутри продакшн-кода. Вынесен в отдельный
[BUG-789](BUG-789-OPEN.md), т.к. требует другого механизма фикса и не описан
строкой BUGS.md этого бага.
