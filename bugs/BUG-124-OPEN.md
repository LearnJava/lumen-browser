# BUG-124

**Статус:** OPEN (DEBTOR)
**Компонент:** layout/paint
**Файл:** `crates/engine/layout/src/box_tree.rs`
**Baseline (KNOWN_DEBTORS):** 1.09%

## Описание

TEST-51 residual 1.09% (thr 0.5%): дробные layout Y-координаты (52.20/72.20/196.20 от h2 line-height 19.2px) vs пиксельное округление Edge. Systemic, affects most tests; root-cause task = PS-1 «pixel snapping единая политика» (reserved by P1 2026-06-10, STATUS-P1.md). Re-run TEST-51 after PS-1 lands.

## Триаж 2026-06-25 (тест через step37) — paint-снэппинг исключён

Гипотезу «1px AA-кайма от дробной кромки, чинится снэппингом в paint» **проверили
эмпирически и опровергли** (step37, 3 раунда):

| Раунд | Правка (femtovg backend) | Результат |
|---|---|---|
| 1 | `snap_rect` → `FillRect`/`FillRoundedRect`/`DrawBorder` | 1.09% → **1.17%** |
| 2 | + снэп scissor-клипов (`PushClipRect`/`PushScrollLayer`) | → **1.13%** |
| 3 | откат (вердикт) | — |

Решающая улика (сэмплы верхней кромки 1-го бокса, scale=1.0):

```
 y    Edge          Lumen         вердикт
 195  border(15,52,96)  bg(26,26,46)   Edge уже бордер, Lumen ещё фон
 196  border         border         совпало
 197  content(83,52,131) border       Edge уже контент, Lumen ещё бордер
 198  content        content        совпало
```

**Бокс у Edge стоит ровно на 1 device-пиксель ВЫШЕ, чем у Lumen.** Дисплей-лист
Y=196.20, Lumen снэпает→196, Edge держит ~195: это расхождение НАКОПЛЕННОЙ позиции
в потоке (line-height 19.2px / half-leading / block-advance), а не AA-кайма на
дробной кромке. Paint-снэппинг бессилен — бокс уже на неверном целом Y до paint.

**Вывод:** не точечный paint-фикс и не P3-багфикс. Реальный фикс = (a) выровнять
раскладку line-box/half-leading с Edge, чтобы дробная позиция совпала, **+** (b)
единый pixel-snap позиций на этапе layout (= PS-1). Затрагивает все тесты →
валидация полным `run.py --continue-on-fail --build`. Домен P1. Помечен
KNOWN_DEBTOR 1.09% до приземления PS-1.
