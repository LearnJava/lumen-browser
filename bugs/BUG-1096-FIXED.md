# BUG-1096 — `compute_referrer` не обрезает `Referer` длиннее 4096 байт до origin (Referrer Policy §8.3)

**Статус:** FIXED 2026-09-23 (P3)
**Тип:** дефект реализованного кода — `compute_referrer` реализует шаги policy/downgrade §8.3, но пропускает шаг «if `result`'s length is greater than 4096, set `result` to `result`'s origin»
**Область:** network (`crates/network/src/referrer_policy.rs:98-148`, `compute_referrer`)
**Владелец:** P1/P3 (`lumen-network`)
**Заведён:** 2026-09-22 (WPT-RUN-7 срез 54, `referrer-policy/4K`/`4K+1`/`4K-1` baseline)

## Симптом

Когда referrer-URL самой страницы длиннее 4096 байт (страница сама себя
паддит через `history.replaceState`, ровно то, что тестирует WPT-семья
`referrer-policy/4K*`), `compute_referrer` при любой политике, дающей
`full()` для same-origin (`no-referrer-when-downgrade`/`origin-when-cross-origin`/
`same-origin`/`strict-origin-when-cross-origin`/`unset` (дефолт `strict-origin-when-cross-origin`)/`unsafe-url`),
подставляет в `Referer` **всю** строку 4096+ байт как есть, вместо того
чтобы сначала обрезать её до origin.

## Прямое измерение

`tests/wpt/referrer-policy/{4K,4K+1,4K-1}` (WPT-RUN-7 срез 54, `--update-expected --all --root referrer-policy/4K[+1|-1] --recursive --processes 7 --binary target/dev-release/lumen.exe`, dev-release, Windows):
**207 подтестов** семьи `4K*` (`xhr`/`fetch`-варианты, `same-http`/`same-https`
origin) падают на

```
assert_in_array: document.referrer value "http://localhost:18300/referrer-policy/4K/gen/.../xhr.http.html?<паддинг до 4096+ байт>"
  not in array ["http://localhost:18300/", undefined]
```

— движок отправил непокоцанный URL там, где спека требует `result`'s origin
(`"http://localhost:18300/"`).

## Причина

`compute_referrer` (`crates/network/src/referrer_policy.rs:98-148`) реализует
`is_downgrade`/`same_origin` ветвление §8.3, но не содержит шага длины:
ни в одной ветке `match policy` нет проверки
`referrer_url.as_str().len() > 4096` перед вызовом `full()`. `full()`
(`referrer_policy.rs:120`) склеивает origin с `path_and_query()` без проверки
итоговой длины.

## Направление починки (не предписание)

Добавить шаг после вычисления `full()`/перед возвратом: если
`full().len() > 4096`, вернуть `origin_only()` вместо `full()` — независимо
от того, same-origin результат или нет (шаг общий для всех политик, идёт
ПОСЛЕ policy-ветвления в спеке, не внутри него). Юнит-тест по образцу
существующих в этом файле (`strict_origin_when_cross_origin_*`) с
искусственно длинным `referrer_url`.

## Как проверить фикс

`tests/wpt/run_report.py --check --all --root referrer-policy/4K --recursive --processes 7` —
207 подтестов `assert_in_array: document.referrer value` (семьи `4K*`) должны
перейти в unexpected PASS (сузить `.ini`, `--update-expected` заново).

## Исправлено

`compute_referrer` (`crates/network/src/referrer_policy.rs:98-153`) теперь
применяет спековый шаг длины ПОСЛЕ `match policy` (как и требует §8.3 —
шаг общий для всех политик, не внутри ветвления): результат каждой ветки
собирается в `result: Option<String>`, и на выходе

```rust
result.map(|r| if r.len() > 4096 { origin_only() } else { r })
```

заменяет любой `result` длиннее 4096 байт на origin, независимо от того,
`full()` или уже `origin_only()` его произвёл.

Добавлены 2 юнит-теста: `full_referrer_over_4096_bytes_collapses_to_origin`
(4096+ байт → origin) и `full_referrer_under_4096_bytes_stays_full` (короткий
путь остаётся нетронутым). `cargo clippy -p lumen-network --all-targets --
-D warnings` чист, `cargo test -p lumen-network referrer_policy` 11/11.

Живой WPT-прогон (`tests/wpt/run_report.py --check --root referrer-policy/4K`,
207 подтестов) в этой сессии не переверифицирован — фикс подтверждён на
уровне спеки и юнит-теста; `.ini`-сужение и unexpected-PASS проверка
оставлены следующему прогону WPT-RUN-7.
