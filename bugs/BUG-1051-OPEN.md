# BUG-1051 — вьюпорт-/шрифт-относительные единицы дают другой рендер-размер, чем их «root-relative» аналог, под `zoom`

**Статус:** OPEN
**Заведён:** 2026-09-13 (BUG-532 срез P3, живой прогон `css/css-viewport/zoom/relative-units.html` и `zoom/font-relative-units.html`)
**Область:** layout (используемое значение — `getBoundingClientRect()`, не CSSOM)
**Владелец:** P1/P3

## Симптом

Оба файла сравнивают локальную относительную единицу с её «root-relative»
парой на одном и том же элементе под одинаковым эффективным `zoom` —
спека требует равенства.

`run_smoke.py //css/css-viewport/zoom/font-relative-units.html`
(dev-release, 2026-09-13) — 1/6 (только `ic = ric` проходит):

```
FAIL em = rem - expected 80 but got 200
FAIL lh = rlh - expected 992 but got 240
FAIL ex = rex - expected 992 but got 109.1796875
FAIL cap = rcap - expected 992 but got 140
FAIL ch = rch - expected 992 but got 126.171875
```

`run_smoke.py //css/css-viewport/zoom/relative-units.html` — 1/6:

```
FAIL relative-units 4 - vh in outside expected 15.84 +/- 1 but got 7.919999599456787
FAIL relative-units 5 - vw in outside expected 20.48 +/- 1 but got 10.239999771118134
```

`vh`/`vw` под зумом дают ровно половину ожидаемого (похоже на двойное
масштабирование в одну и обратную сторону — фактор `2`, тестовый `zoom`
файла тоже `2`, совпадение подозрительное, не проверено). `lh`/`ex`/`cap`/
`ch` расходятся сильнее и без очевидного целочисленного фактора.

## Что дальше

Не локализовано глубже прогона. Первый шаг — проверить, резолвятся ли
`lh`/`ex`/`cap`/`ch`/`vh`/`vw` против уже зумленного или незумленного
базиса на местах их использования (аналогично тому, как `em`/`rem`
разбирались при реализации `effective_zoom`, 2026-08-10) — вероятный
кандидат тот же класс проблемы, что и `font_size`'s
`FontSizeBasis::Absolute`/`ParentRelative` различие, просто ещё не
воспроизведённое для этих единиц.

## Срез 1 (P6, 2026-09-30) — `vh`/`vw`/`vmin`/`vmax`

Причина: `zoom_length` (`cascade.rs`) масштабировал только `Px`; вьюпорт-единицы
резолвятся от **незумленного** вьюпорта, а комментарий утверждал обратное.
Исправлено: коэффициент зума умножается на коэффициент `Vh/Vw/Vmin/Vmax`;
тест `zoom_scales_viewport_unit_coefficients`. `relative-units.html` 4/5 → ожидается зелёный.

## Остаток (найден пробой `--dump-layout`, html `font-size:20px; zoom:2`)

- ~~`rem` — константа 16~~ — исправлено срезом 2 (ниже).
- `rlh`/`rex`/`rch`/`rcap` не парсятся (декларация отбрасывается).
- `lh` = `Em(1.2)` (`calc.rs:1052`), `cap` = `Em(0.7)` — заглушки, не line-height/cap-height.
- `ex`/`ch` берут метрики шрифта элемента — при верном зумленном `font_size` расхождения
  быть не должно, пока не заведены `rex`/`rch`.

## Срез 2 (P6, 2026-09-30) — `rem` следует за корнем

`ComputedStyle::root_font_size` (наследуется, ставится на `<html>` после font-size). `font-size`/`line-height` в `rem` считаются от него; в боксовых длинах (`width`/`margin`/`padding`/`gap`/`top…`) коэффициент `Rem` домножается на `root_font_size/16` в `apply_zoom_to_lengths` (на самом корне — на `zoom`). Не охвачено: `rem` внутри `calc()`, прочие свойства с `Length::Rem` (`letter-spacing`, `overflow-clip-margin`…). Тест `rem_follows_root_font_size_and_zoom`. Пиксельный гейт не прогнан (TEST-00 из менеджера), `dump_golden` 12/12.

## Срез 3 (P6, 2026-09-30) — `rlh`/`rcap`/`rex`/`rch`/`ric` парсятся

Раньше декларация с этими единицами отбрасывалась. Теперь — кратные корневого шрифта (`Length::Rem`) с теми же Phase 0 коэффициентами, что у пар `lh` 1.2 / `cap` 0.7 / `ex`,`ch` 0.5 (`length.rs`, `calc.rs`). Тест `root_relative_font_units_parse`. Не сделано: настоящие `lh` (computed line-height) и `cap`, метрики `ex`/`ch` от корневого шрифта — пары `lh/rlh`, `ex/rex`, `ch/rch` в WPT могут расходиться. `snapshot_cpu` красный — известный дрейф BUG-1008; `dump_golden` 12/12.

## Срез 4 (P6, 2026-09-30) — настоящий `lh`

`Length::Lh` (раньше `Em(1.2)`): резолвится в used line-height бокса через thread-local `FONT_LH` (`push_lh_context` в `layout_dispatch.rs`, рядом с `ch`/`ex`); вне layout и в `font-size` — фолбэк 1.2em. Тест `length_resolve_lh_uses_line_height_context`, `lumen-layout` 4128 ок, `dump_golden` 12/12. Пиксельный гейт не прогнан. Не сделано: `cap` (нет метрики cap-height в `TextMeasurer`), `ex`/`ch` от корневого шрифта для `rex`/`rch`, `rlh` от корневого line-height. BUG-1051 остаётся OPEN.
