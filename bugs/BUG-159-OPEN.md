# BUG-159

**Статус:** OPEN
**Компонент:** paint
**Файл:** `crates/engine/paint/src/display_list.rs:2542` (`fill_buckets`)

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
