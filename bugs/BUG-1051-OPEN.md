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

- `rem` — константа `ROOT_FONT_SIZE = 16` (`parse/font.rs:233`), а не font-size
  корня: `5rem` = 80 вместо 200. Это не про zoom, `rem` не следует за `html { font-size }` вообще.
- `rlh`/`rex`/`rch`/`rcap` не парсятся (декларация отбрасывается).
- `lh` = `Em(1.2)` (`calc.rs:1052`), `cap` = `Em(0.7)` — заглушки, не line-height/cap-height.
- `ex`/`ch` берут метрики шрифта элемента — при верном зумленном `font_size` расхождения
  быть не должно, пока не заведены `rex`/`rch`.
