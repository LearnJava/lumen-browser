# BUG-1784 — getComputedStyle бокссового элемента отдаёт пустой `display` (`""` вместо `"inline"`)

**Статус:** OPEN
**Заведён:** 2026-11-04 (воркер 1 zshift, найден в проверке проекта задачи VTT-RENDER-S2 — к ней не относится)
**Область:** layout / driver (`crates/driver/tests/cases/bug1191_boxless_computed_style.rs`, getComputedStyle элемента без `LayoutBox`)

## Симптом

Тест `cases::bug1191_boxless_computed_style::boxless_elements_report_full_computed_style`
(введён фиксом [BUG-1191](BUG-1191-FIXED.md), b114c0c4c1) падает на `main`:

```
thread 'cases::bug1191_boxless_computed_style::boxless_elements_report_full_computed_style'
panicked at crates\driver\tests\cases\bug1191_boxless_computed_style.rs:32:5:
assertion `left == right` failed
  left: ""
 right: "inline"
```

Страница теста: пустой `<span id="empty">` в плоском дереве (`#host a<span>b`), плюс
элементы внутри `display:none` и `content-visibility:hidden`. Предшествующие ассерты
`getComputedStyle(...).length > 0` для всех трёх элементов **проходят** — computed style
не пуст, пусто именно значение `display` у `#empty` (до первого падения дело
до `in-none`/`in-cv` не доходит).

## Падает на чистом main — проверено

2026-11-04, слот zs1-work: stash-эксперимент — все правки VTT-RENDER-S2 сняты
(`git stash push`), дерево = `d107c6571` (merge VTT-RENDER-S1) + строка TASKS.md
(69a17ed7d), прогон повторён, правки возвращены (`git stash pop`). Результат
одинаков с правками и без (exit 101): правки S2 (настройки cue, vtt_geometry.rs,
js-пламбинг) не касаются каскада/getComputedStyle.

## Воспроизведение

```
cargo test -p lumen-driver --test all bug1191        # exit 101
```

## Подозрение на виновника (гипотеза, не проверено)

Между фиксом BUG-1191 и текущим HEAD каскад/layout меняли, в частности:
af1cda3ba (FIELDSET-LEGEND-4, 2026-10-28 — правка UA-фазы каскада,
`redo_ua_fieldset_for_writing_mode`) и 46b046133 (background-clip: border-area,
2026-10-27). Какой из них (и какой слой — каскад, `getComputedStyle`, абсолютизация
значений) ломает бокссовый `display` — не установлено, бисект не гонялся.

## Что делать

Бисект по `cargo test -p lumen-driver --test all bug1191`; по виновнику — сверить с
[`BUG-1191-FIXED.md`](BUG-1191-FIXED.md), какой слой обязан отдавать каскадный
`display` элементу без бокса, и восстановить контракт (включая второй тест набора
`boxless_elements_are_still_not_rendered` — пока он зелёный).

## Как проверить фикс

`cargo test -p lumen-driver --test all bug1191` зелёный; повторный stash-прогон
не нужен — падение воспроизводится без чужих правок.
