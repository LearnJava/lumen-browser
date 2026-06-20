# BUG-199

**Статус:** OPEN (DEBTOR — KNOWN_DEBTORS 7.03%)
**Компонент:** layout
**Тест:** TEST-71 (diff 7.03%)

## Описание

`@starting-style`: статический рендер двух цветных блоков отличается от Edge-эталона.

## Расследование (2026-06-20)

Дамп display-list (`--dump-display-list 71-starting-style.html`) — **геометрия и
цвета идеальны**:

```
FillRoundedRect (292.00, 260.00, 200.00, 200.00) #e94560ff r=[12,12,12,12]   ← box-a
FillRoundedRect (532.00, 260.00, 200.00, 200.00) #0f3460ff r=[12,12,12,12]   ← box-b
```

Обе коробки 200×200, корректно отцентрованы (flex center, gap 40), полная
непрозрачность — это **settled-состояние** страницы (как она выглядит после
завершения 0.4s entry-перехода). `@starting-style` корректно НЕ просачивается в
статический каскад (CSS Transitions L2 §3.4): он задаёт *before-change* стиль
только для entry-переходов.

Пиксельный замер Edge-эталона (горизонтальная линия y=360):
- box-a (красный #e94560): полоса x=338..445 → **ширина ~107px**, центр 391 =
  `transform: scale(0.5)` применён (200→100, центр сохранён).
- box-b (синий #0f3460): полоса x=458..657 → левый край 458 vs нормальные 532 =
  `transform: translateX(-80px)` применён.
- Обе коробки при **полной непрозрачности** (точные #e94560/#0f3460, без блендинга).

То есть Edge показывает: **transform у `@starting-style` START-значения** (scale 0.5,
translate -80, ~0% прогресса), но **opacity у END-значения** (1, ~100% прогресса).
Для синхронных 0.4s ease-переходов это взаимно несогласованный кадр — артефакт
тайминга захвата Edge `--headless --screenshot` (без `--virtual-time-budget`),
поймавшего entry-transition в полёте, а не стабильный рендер.

## Почему не P3-фикс

Дефекта движка нет: Lumen рендерит спек-корректное settled-состояние. Чтобы
«совпасть» с Edge, потребовалось бы:
1. Провести `@starting-style` в каскад / TransitionScheduler — **домен P4** (CSS
   at-rules), запрещено для P3;
2. Реализовать полный engine entry-переходов (`starting_style.rs` сейчас stub);
3. Воспроизвести взаимно несогласованный кадр (transform START + opacity END) с
   точным таймингом захвата Edge — невозможно.

Тот же класс, что TEST-77/BUG-126: «Lumen спек-корректнее Edge; расхождение в
reference-браузере, не дефект движка».

## Резолюция

- TEST-71 → `KNOWN_DEBTORS` в `graphic_tests/run.py` (baseline 7.03%, anchor BUG-199).
- Регресс-тест `starting_style_does_not_leak_into_static_cascade`
  (`crates/engine/layout/src/style.rs`) — фиксирует спек-корректный инвариант:
  `@starting-style` opacity/transform НЕ просачиваются в статический каскад.
- BUG-199 остаётся OPEN как deferred-фича (entry-transition engine).
