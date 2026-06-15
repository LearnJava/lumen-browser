# BUG-159

**Статус:** FIXED 2026-06-15
**Компонент:** paint
**Файл:** `crates/engine/paint/src/display_list.rs:2542` (`fill_buckets`)

## Фикс (2026-06-15)

Корень уточнён: дефект возникает, только когда scroll-контейнер **не** owns
stacking context (плоский `overflow:auto`/`scroll`, `position:static`, без
z-index/opacity/transform). Такой контейнер эмитит `PushScrollLayer`/
`PopScrollLayer` inline в `contents` текущего SC; их `Pop` закрывается до того,
как любой потомок, создающий собственный stacking context (z-indexed, opacity,
transform, …), рисуется в более позднем слоте painting order — потомок сбегал и
из scroll-клипа, и из scroll-translate. Это шире `position:absolute`: затрагивает
любой own-SC потомок. (Если scroll-контейнер сам owns SC, он оборачивает потомков
через `root_bg`/`post` — `PushScrollLayer` в фазе RootBackground, `PopScrollLayer`
в CloseLayer после всех детей, — и дефекта нет.)

Решение зеркалит clip-наследование BUG-131: в non-SC ветке `fill_buckets` теперь
наследуем и `PushScrollLayer` (не только rect-клипы) в цепочке для дочерних SC, а
SC-root переустанавливает его как внешний слой (`clip_pop_for` уже отображал
`PushScrollLayer → PopScrollLayer`). `position:fixed`/`sticky` потомки scroll-слой
**не** наследуют (фильтруются перед рекурсией) — fixed привязан к viewport, sticky
имеет собственную scroll-aware машинерию.

Ограничение: потомок с собственным `transform` по-прежнему не скроллится —
`PushTransform` в femtovg использует `set_transform` (заменяет матрицу page-space,
стирая scroll-translate). Это предсуществующее ограничение трансформ-под-скроллом,
ортогонально данному фиксу.

Регресс-тесты (`display_list.rs`):
`ordered_zindexed_child_scrolls_with_overflow_auto_ancestor`,
`ordered_fixed_child_does_not_inherit_ancestor_scroll_layer`.
CPU snapshot gate байт-нейтрален (проверено сравнением с чистым main).

## Описание

Абсолютно-позиционированный потомок scroll-контейнера эмитится в display list
**после** `PopScrollLayer` корневого scroll-слоя — то есть вне scroll-слоя, как
если бы элемент был `position:fixed`. Из-за этого `position:absolute`-элемент не
скроллится вместе со страницей (рендерер применяет `-scroll_y` только к контенту
внутри scroll-слоя).

Воспроизводится на `https://lenta.ru/`: тёмная шапка `#292929` —
`position:absolute` с containing block = in-flow `position:relative` обёртка
внутри body (`overflow:auto`). В display list:

```
2:   PushScrollLayer clip=(0,0,1024,7831) scroll=(0,0)   ← корневой scroll body
...
833: PopScrollLayer                                       ← закрытие корневого слоя
834: FillRect (0,0,1280,270) #292929ff                    ← abs-шапка ВНЕ слоя
```

Для `position:fixed` (сайдбар, fixed-оверлеи) отрисовка после `PopScrollLayer` —
корректна. Для `position:absolute` с in-flow containing block — нет: при скролле
страницы шапка останется приклеенной к верху вместо того, чтобы уехать.

## Воспроизведение

```bash
./target/debug/lumen.exe --dump-display-list https://lenta.ru/ 2>/dev/null > /tmp/dl.txt
grep -nE "PushScrollLayer|PopScrollLayer" /tmp/dl.txt   # корневой слой 2..833
grep -nE "FillRect .*1280.00, 270" /tmp/dl.txt          # шапка на строке 834 (> 833)
```

## Старт расследования

В `fill_buckets` / порядке отрисовки разделить out-of-flow по типу: `fixed` —
вне scroll-слоя (как сейчас), `absolute` со scroll-контейнером в цепочке
containing block — внутри соответствующего scroll-слоя. Проверить, как stacking
phase для positioned z-auto взаимодействует с push/pop scroll-слоя
(`crates/engine/paint/src/display_list.rs`).

Замечание: визуально вторично — на scroll=0 шапка видна корректно; дефект
проявляется только при прокрутке.
