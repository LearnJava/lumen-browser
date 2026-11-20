# BUG-1834-OPEN: `cargo test -p lumen-shell --bin lumen` падает STATUS_HEAP_CORRUPTION после `platform::audio_player`

- **Статус:** OPEN
- **Область:** shell test-binary (`crates/shell/src/bin` тесты; 2589 тестов, падение в районе `platform::*`)
- **Приоритет:** P3 (краш тестового бинаря; в продакшн-коде не воспроизводился, источник порчи кучи не локализован)

## Симптом

`cargo test -p lumen-shell --bin lumen` дважды подряд падает с
`exit code: 0xc0000374, STATUS_HEAP_CORRUPTION` на одном и том же месте —
сразу после `platform::audio_player::tests::probe_duration_mp3_is_finite`
(следующие по списку — `platform::clipboard::windows_roundtrip`,
`platform::msaa_wndproc`, `platform::screen_capture::windows_impl`,
`platform::os_params::windows`).

- Первый прогон — внутри `scripts/scoped-test.sh` (бинарь `lumen-9e72f88a…`),
  второй — отдельным `cargo test -p lumen-shell --bin lumen`
  (бинарь `lumen-d4cbb3c6…`, другая feature-резолюция) — краш в одной точке.
- `platform::`-подмножество отдельно (`--test-threads=4`, 52 теста) — зелёное:
  порча происходит раньше по ходу параллельного прогона, а CRT-детектор
  срабатывает позже. Источник может быть любым unsafe/FFI-тестом до этого места.
- К диффу FINGERPRINT-MANIFEST отношения не имеет: изменения в shell — четыре
  call-site'а `with_content_decoders(default_content_decoders())` в
  page_source/resource_base/download/update, чистый builder-код без unsafe.

## Воспроизведение

```
cargo test -p lumen-shell --bin lumen
# … crash 0xc0000374 после platform::audio_player::tests::probe_duration_mp3_is_finite
```

## Актуальность (2026-11-20, main `c6d618cf9`)

Третий прогон того же бинаря — **без краша**: 2567 passed / 1 failed
(`page_load::css_pending::tests::deadline_releases_pending_once` — тайминговый
флак на дедлайне 1999 мс, к теме бага не относится). `platform::`-подмножество
зелёное и в прошлый раз. Два краша подряд при заполненном на 100 % диске D:
и параллельной компиляции — правдоподобная env-причина (pagefile/память);
доказательств дефекта в коде нет. Если следующий полный прогон снова упадёт
0xc0000374 — возвращаться к бисекту; иначе закрывать как env.

## Гипотеза

Unsafe/FFI в platform-слое (audio capture/player, msaa_wndproc, screen_capture,
wgpu/femtovg-пути в ранних тестах) либо внешняя причина (драйвер, AV). Нужен
бисект по тестам (`--test-threads=1` + прогон префиксов) или прогон под
PageHeap/ASAN-аналогом; сначала проверить, воспроизводится ли на чистом main
без локальных диффов.

Заведён P3 при гейте FINGERPRINT-MANIFEST (scoped-test), 2026-11-20.
