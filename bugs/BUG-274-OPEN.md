# BUG-274 — wgpu-бэкенд: idle CPU ~4× выше femtovg + разовый скачок памяти ~450 МБ после старта

**Статус:** OPEN — найден по CPU/RSS профилю, корень не диагностирован (только гипотезы)
**Компонент:** paint (`WgpuBackend`, `crates/engine/paint/src/backends/wgpu_backend.rs`) / shell (event loop для wgpu-пути, `backend_factory.rs`)
**Найден:** 2026-07-08, сравнительный замер `LUMEN_BACKEND=femtovg` vs `LUMEN_BACKEND=wgpu` по запросу пользователя («можно ли уже запустить браузер на wgpu и будет ли это быстрее»)

## Симптом

На идентичной статичной странице (`graphic_tests/1000000-final.html`, 2002 DOM-узла, 1062
paint-команды) wgpu-бэкенд в простое ест заметно больше CPU, чем femtovg, и один раз резко
наращивает память вскоре после открытия окна — ещё до какого-либо взаимодействия.

## Данные (dev-release, `target/dev-release/lumen.exe`, один прогон на бэкенд)

### CPU в простое (10 с, страница статична, без ввода)

| Бэкенд | CPU-время за 10с | ≈% одного ядра |
|---|---|---|
| femtovg | 375 мс | 3.8% |
| wgpu | 1422 мс | 14.2% |

### CPU во время скролла (~4.2 с, 16×PageDown)

| Бэкенд | CPU-время |
|---|---|
| femtovg | 203 мс |
| wgpu | 62.5 мс |

(На скролле wgpu, наоборот, эффективнее — проблема именно в простое, не в самом рендере при движении.)

### Память — покадровые чек-поинты (PowerShell `Get-Process`, снято до/после каждого PageDown)

wgpu:
```
t=5s  (сразу после загрузки)      WorkingSet=763.6MB PrivateBytes=741.7MB
t=10s (idle settle, БЕЗ ввода)    WorkingSet=1034.9MB PrivateBytes=1015.7MB   ← скачок +274MB
после PGDN #1..#8, скролл назад   WorkingSet=1034.9MB PrivateBytes=1015.7MB   ← дальше плоско
```

femtovg (тот же протокол):
```
t=5s   WorkingSet=759.4MB PrivateBytes=562.2MB
t=10s  WorkingSet=759.4MB PrivateBytes=562.1MB   ← без изменений
после PGDN #1..#8, скролл назад   PrivateBytes=562.1MB   ← плоско всё время
```

`GPU Process Memory\Local Usage` (Get-Counter, снято после idle-окна): femtovg 559.1MB, wgpu 709.3MB.

## Важное уточнение (снимает гипотезу утечки)

Рост памяти wgpu происходит **разово, между t=5с и t=10с, до первого PageDown** — то есть не
связан со скроллом и не прогрессирует дальше при взаимодействии (8 шагов вперёд + 8 назад —
без изменений). Это не утечка, а единовременная стоимость чего-то, что происходит в первые
секунды после появления окна.

## Гипотезы (не проверены — нужен профиль)

1. **Idle CPU (4×).** У wgpu-пути, вероятно, нет аналога фикса BUG-271 (протухший
   `WaitUntil` / безусловный rAF-пинг `request_redraw`) — либо тот фикс общий для обоих
   бэкендов на уровне шелла, и источник горения именно в `WgpuBackend::render()`/его
   presentation-цикле (например, лишний re-submit кадра или poll GPU-адаптера каждую
   итерацию event loop).
2. **Разовый скачок памяти (+274 МБ WS / +450 МБ private суммарно от старта).** Похоже на
   отложенную инициализацию: компиляция шейдеров/pipeline-кэш wgpu, преаллокация GPU-буферов
   или атласа текстур, случающаяся не в момент создания бэкенда, а чуть позже (первые кадры
   после появления окна).

## Что НЕ проверено (следующие шаги для того, кто возьмёт баг)

- Есть ли у `WgpuBackend` поддержка `LUMEN_FRAME_LOG` (см. BUG-273) — если нет, добавить,
  чтобы разбить idle-CPU по типам работы за кадр.
- Профиль на других страницах (JS-тяжёлых, с анимациями) — на одностраничном статике эффект
  может отличаться от реальных сайтов.
- Влияние профиля сборки (`dev-release` без LTO vs `release`) на абсолютные цифры —
  относительное сравнение (4× CPU, +450MB) должно быть профиль-независимым, но не проверено.
- Не разделено, один это дефект или два разных (idle-CPU и memory-spike могут иметь разные
  причины) — объединены в один тикет, т.к. найдены в одном прогоне.

## Как воспроизвести

```powershell
$env:LUMEN_BACKEND="femtovg"; $env:LUMEN_MEM_REPORT="1"
target\dev-release\lumen.exe graphic_tests\1000000-final.html

$env:LUMEN_BACKEND="wgpu"; $env:LUMEN_MEM_REPORT="1"
target\dev-release\lumen.exe graphic_tests\1000000-final.html
```

Снять `Get-Process -Id <pid> | Select TotalProcessorTime,WorkingSet64,PrivateMemorySize64` до и
после 10-секундного простоя (без фокуса/ввода) — сравнить дельту CPU-времени и память на
чек-поинтах t=5s / t=10s.

## Контекст

Найдено не как таргетированный баг-хант, а как побочный результат исследования «стоит ли
переводить live-рендер шелла на wgpu вместо femtovg» — см. `docs/decisions/ADR-010`
(миграционный путь бэкендов) и `subsystems/paint.md`. До закрытия этого бага wgpu как
дефолтный live-путь брать не стоит — простой на статичной странице должен стоить ~0 CPU
(инвариант, закреплённый BUG-271), а на wgpu он нарушен.

## Обновление 2026-07-13 — P1-wgpu-vkgl: замер с новыми фичами

До этого коммита wgpu на Windows собирался только с фичей `dx12`; `backend_probe` (Vulkan→GL→DX12)
всегда откатывался на DX12 — кандидаты Vulkan и GL были недоступны на уровне wgpu-runtime.
С P1-wgpu-vkgl Windows-сборка включает `["dx12", "vulkan", "gles", "wgsl", "std"]`.

### Результаты probe на Intel Iris Plus (2026-07-13)

```
[probe] Vulkan: present=WHITE texture=ok  adapter="Intel(R) Iris(R) Plus Graphics" — отклонён
[probe] GL:     present=WHITE texture=n/a adapter="Intel(R) Iris(R) Plus Graphics" — отклонён
[probe] DX12:   present=ok    texture=ok  adapter="Intel(R) Iris(R) Plus Graphics" — ПРИНЯТ
[probe] бэкенд выбран за 2127 мс: DX12
```

Выводы:
- **Vulkan** — BUG-275 подтверждён (`present=WHITE`, DWM-заголовок исправен).
- **GL (GLES через wgpu/WGSL)** — тоже `present=WHITE`; это wgpu-over-GLES, не femtovg-over-GL —
  драйвер Intel Iris Plus не презентует wgpu-GLES-swapchain через DWM. `texture=n/a` означает
  отсутствие COPY_SRC у GLES-поверхности.
- **DX12** — единственный рабочий путь на этой машине.

Данные из exp-ветки о "wgpu-GL как лучшем" применимы к другой машине или другой конфигурации
GLES-бэкенда — на Intel Iris Plus он также white-screens.

### Замер idle CPU (dev-release, график. 1000000-final.html, t=5..15 с)

| Бэкенд | Дельта CPU за 10 с | % одного ядра | Примечание |
|---|---|---|---|
| femtovg (new) | ~219 мс | ~2.2% | базовая линия |
| wgpu/DX12 probe (new, Phase 2) | ~2391 мс | ~23.9% | probe 2.1 с overhead; blit per-frame |
| wgpu/DX12 (original BUG-274, pre-Phase2) | ~1422 мс | ~14.2% | |

Wgpu DX12 ухудшился vs исходного замера (~24% vs 14%). Гипотеза: scroll-compositor (Phase 2,
M3 blit path) делает GPU-blit каждый кадр даже в idle (`[frame] delta Identical` → `[frame] band blit`),
тогда как skip-identical-frame только предотвращает полный repaint, но не сам blit. Фикс = добавить
skip-blit когда `delta Identical` и нет dirty-regions (отдельная подзадача).
