# BUG-192

**Статус:** OPEN (DEBTOR)
**Компонент:** paint
**Тест:** TEST-55 (KNOWN_DEBTOR 0.54%, класс BUG-128)

## Описание

`<video>` replaced element — поведение placeholder для `<video src="nonexistent.mp4">`
(нет загружаемого источника, нет poster, нет декодированного кадра).

## Расследование (2026-06-24)

Фича рендерится **корректно** — реального дефекта движка нет.

- **Edge** рисует `<video>` без загружаемого источника как **прозрачный бокс** (серого
  placeholder нет, виден только фон страницы). Видна только рамка у бокса с `border`.
- **Lumen** рендерит так же: пустое `<video>` без poster и без GIF-кадра не эмитит
  `DrawImage` (BUG-097, `display_list.rs` `BoxKind::Video` ветка — `is_gif_src=false` и
  `poster.is_empty()` → ничего не рисуется). Описание в `run.py` про «grey DrawImage
  placeholder» устарело: фактическое поведение совпадает с Edge.
- Бокс с `border: 3px solid #4299e1` (200×120, box-sizing border-box) совпадает с Edge
  по размеру и цвету рамки (`--dump-layout`: rect 361,212 200×120, bw 3px, bc #4299e1).

### Декомпозиция diff (CPU/ffmpeg-free, 0.54%)

| Доля | Источник |
|---|---|
| ~90% (0.48%) | font-parity 6 меток `.label` (11px sans-serif, Inter vs Edge) — rule 3 |
| ~0.05% | бордер-бокс на 1px ниже: Lumen y=212.06, Edge y=211 |

1px-сдвиг бордер-бокса (ряд 2) вызван высотой строки метки в ряду 1: row2_top =
row1_top(25) + row1_height + margin(16). Lumen row1_height = 171.06, Edge = 170 —
разница в line-box метки 11px-шрифта (Inter «normal» ≈1.2 vs Edge sans). То есть и
этот остаток — font-parity (BUG-128), а не геометрический дефект.

## Резолюция

KNOWN_DEBTOR: `'55': ('BUG-192', 0.54)` в `graphic_tests/run.py`. Весь остаток —
font-parity (rule 3); закроется только с общим переходом на Edge-совместимый шрифт
(класс BUG-128). Запись держится OPEN как требует механизм KNOWN_DEBTORS.
