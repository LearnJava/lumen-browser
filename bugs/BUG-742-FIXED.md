# BUG-742: процентная `width` в intrinsic-расчёте стирала вклад содержимого

**Статус:** FIXED 2026-08-10
**Компонент:** layout (`crates/engine/layout/src/box_tree.rs` —
`preferred_inline_block_width`, `max_content_outer_width`,
`min_content_outer_width`)
**Найден:** P3, при разборе пункта 2 [BUG-733](BUG-733-OPEN.md), 2026-08-10

## Симптом

Любой shrink-to-fit бокс (`inline-block`, `inline-flex`, float,
flex-элемент с `width: auto`), внутри которого лежит потомок с процентной
`width`, схлопывался до собственных padding + border этого потомка — всё
поддерево ниже переставало влиять на ширину.

На `https://www.tbank.ru/` этим убивалась кнопка CTA «Оформить карту»:

```
div.fbbDUwSMn   display:flex   1104px
└ div.gbbDUwSMn                 32 ← должно быть ~160
  └ h4           inline-block    32
    └ a.abwTheaUM inline-flex    32
      └ span.bbwTheaUM display:flex; width:100%; padding:16px; box-sizing:border-box
        └ span.dbwTheaUM inline-flex; padding:2px 4px
          └ span «Оформить карту»   0 ← текст не влиял ни на что
```

32 = ровно `padding-left + padding-right` у `span.bbwTheaUM`. Кнопка не
«отсутствовала» (формулировка исходной заявки), а была схлопнута в жёлтую
полоску 32 px шириной.

## Механизм

Все три функции intrinsic-ширины начинались с ветки «явная CSS `width`
побеждает содержимое» и резолвили длину с `percent_basis: Some(0.0)`:

```rust
if let Some(w_len) = &s.width
    && let Some(w) = w_len.resolve(em, Some(0.0), viewport)
{ … return outer }
```

`Length::Percent` при базе `Some(0.0)` даёт `Some(0.0)` — то есть процент
считался *явной* шириной, равной нулю, и функция возвращала
`0 + padding + border`, ни разу не заглянув в детей.

По CSS Sizing L3 §5.2.1 процент, который в intrinsic-контексте разрешить не
от чего, ведёт себя как `auto`: вклад берётся из содержимого, а сам процент
разрешается позже, уже на раскладке, от полученной ширины. Ровно так делает
Edge (сверено, таблица ниже).

## Фикс

`percent_basis: Some(0.0)` → `None` в тех же трёх ветках. `Length::Percent`
при `None` возвращает `None` (и `calc()` с процентом внутри — тоже, `?`
пробрасывает), условие `if let` не срабатывает, расчёт уходит в
содержимое. Абсолютные длины (`px`/`em`/`vw`/…) от базы не зависят и
по-прежнему выигрывают у содержимого.

Одна строка на функцию; ни одна другая величина в этих функциях не тронута —
padding, margin и `column-gap` как резолвились против нуля, так и резолвятся
(это отдельное и намеренное приближение, см. комментарий у
`flex_row_intrinsic_sum`).

## Проверка

Восемь форм, `getBoundingClientRect` печатает сама страница
(`.tmp/pct-intrinsic.html`), эталон — headless Edge:

| # | Форма | Edge | Lumen до | Lumen после |
|---|---|---|---|---|
| a | `inline-block > block{width:100%}` + текст | 79×19 | 0 | 79.1×19.2 |
| b | то же + `padding: 0 16px; border-box` | 111×19 | 32 | 111.1×19.2 |
| c | форма кнопки CTA (`inline-flex > flex{100%}`) | 164×55 | 32 | 163.6×55.2 |
| d | `float > block{width:100%}` | 87×19 | 0 | 87.2×19.2 |
| e | `inline-block > block{width:50%}` | 100/50 | 0 | 100.5/50.2 |
| f | `inline-block > block{width:200px}` | 200×19 | 200 | 200×19.2 |
| g | тесный родитель 60px, внутри `width:100%` | 60×38 | 60 | 60×38.4 |
| h | `inline-flex` из двух элементов, первый `width:100%` | 117/65 | 32 | 116.5/64.9 |

Живая страница после фикса: кнопка CTA `160×56`, текст «Оформить карту»
120 px (было `32×56`, текст нулевой ширины).

Регрессии: `--dump-display-list` по всем 158 страницам `graphic_tests/`
совпадает с baseline побайтово (кроме недетерминированной строки загрузки
картинки в `90-avif-image`), `dump_golden.py` — 12/12 PASS, `cargo test -p
lumen-layout` — 3547 + 71 + 1 зелёных. То есть на тестовом корпусе форма
«процент внутри shrink-to-fit» не встречается вовсе, а на живых страницах
встречается на каждой кнопке.

Тесты: `bug742_*` в `box_tree.rs` (6 штук) — вклад содержимого, padding
поверх содержимого, разрешение процента на раскладке, `calc()` с процентом,
неприкосновенность абсолютной `width`, тесный containing block.
